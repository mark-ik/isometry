//! The generator lane: packs, catalog, and the sandboxed runtime.
//!
//! A pack is authored Lua plus its manifest; the catalog is what the host
//! loaded. Execution is bounded (fuel and instruction limits) because packs are
//! third-party content, not first-party code.
//!
//! Split out of `lib.rs` on 2026-07-24; behavior unchanged.

use super::*;

impl GeneratorCatalog {
    pub fn discover<I, P>(roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut candidates = Vec::new();
        let mut diagnostics = Vec::new();
        for root in roots {
            let root = root.as_ref();
            if root.join(GeneratorPack::MANIFEST_FILE).is_file() {
                candidates.push(root.to_path_buf());
                continue;
            }
            match std::fs::read_dir(root) {
                Ok(entries) => {
                    let mut children: Vec<PathBuf> = entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|path| path.join(GeneratorPack::MANIFEST_FILE).is_file())
                        .collect();
                    children.sort();
                    candidates.extend(children);
                }
                Err(error) => diagnostics.push(format!(
                    "read generator-pack root {}: {error}",
                    root.display()
                )),
            }
        }
        let mut packs = Vec::new();
        let mut ids = BTreeSet::new();
        for candidate in candidates {
            match GeneratorPack::load(&candidate) {
                Ok(pack) if ids.insert(pack.manifest().id.clone()) => packs.push(pack),
                Ok(pack) => diagnostics.push(format!(
                    "duplicate content-pack id {} at {}",
                    pack.manifest().id,
                    candidate.display()
                )),
                Err(error) => diagnostics.push(error),
            }
        }
        Self { packs, diagnostics }
    }

    pub fn choices(&self) -> Vec<GeneratorChoice> {
        self.packs
            .iter()
            .flat_map(|pack| pack.manifest().generator_choices())
            .collect()
    }

    /// The table's whole beat vocabulary, gathered from every discovered pack:
    /// `(name, emote label, stylesheet)`.
    ///
    /// A later pack declaring a beat name an earlier one already used wins, so a
    /// campaign restyles the swing simply by shipping its own `strike` -- no app
    /// change, no recompile, which is the point of putting it here.
    ///
    /// A pack whose stylesheet will not open is *skipped with a diagnostic*
    /// rather than failing the table: a missing beat costs an animation, and no
    /// rule may read a beat, so a board with no choreography still plays a
    /// correct game. That is only safe because beats are representation.
    pub fn choreography(&self) -> (Vec<LoadedBeat>, Vec<String>) {
        let mut beats: Vec<LoadedBeat> = Vec::new();
        let mut diagnostics = Vec::new();
        for pack in &self.packs {
            for entry in &pack.manifest().choreography {
                let path = pack.root.join(&entry.style);
                let css = match std::fs::read_to_string(&path) {
                    Ok(css) => css,
                    Err(error) => {
                        diagnostics.push(format!(
                            "beat '{}' in pack '{}': read {}: {error}",
                            entry.name,
                            pack.manifest().id,
                            path.display()
                        ));
                        continue;
                    }
                };
                let loaded = LoadedBeat {
                    name: entry.name.clone(),
                    emote: entry.emote.clone(),
                    css,
                };
                match beats.iter_mut().find(|b| b.name == loaded.name) {
                    Some(existing) => *existing = loaded,
                    None => beats.push(loaded),
                }
            }
        }
        (beats, diagnostics)
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn generate(
        &self,
        record_id: impl Into<String>,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
        limits: GeneratorLimits,
    ) -> Result<GenerationRecord, String> {
        let pack = self
            .packs
            .iter()
            .find(|pack| pack.manifest().generator(&request.generator).is_some())
            .ok_or_else(|| format!("no loaded pack declares generator {}", request.generator))?;
        pack.generate(record_id, request, tape, limits)
    }
}

impl GeneratorPack {
    pub const MANIFEST_FILE: &'static str = "isometry-pack.json";

    /// Load a pack directory and validate its manifest before any generator
    /// assets are read. The canonical root also prevents a declared symlink
    /// from escaping the pack when the asset is opened.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, String> {
        let root = dir
            .as_ref()
            .canonicalize()
            .map_err(|error| format!("open content-pack root: {error}"))?;
        let manifest_path = root.join(Self::MANIFEST_FILE);
        let manifest_json = std::fs::read_to_string(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
        let manifest: ContentPackManifest = serde_json::from_str(&manifest_json)
            .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
        manifest
            .validate()
            .map_err(|error| format!("validate {}: {error}", manifest_path.display()))?;
        Ok(Self { root, manifest })
    }

    pub fn manifest(&self) -> &ContentPackManifest {
        &self.manifest
    }

    /// Load a bounded runtime for the generator named by a fully-qualified
    /// request id such as `demo:forge_item`.
    pub fn runtime_for(
        &self,
        request: &GeneratorRequest,
        limits: GeneratorLimits,
    ) -> Result<GeneratorRuntime, String> {
        let entry = self.manifest.generator(&request.generator).ok_or_else(|| {
            format!(
                "generator is not declared by this pack: {}",
                request.generator
            )
        })?;
        let script = self.read_asset(&entry.script)?;
        GeneratorRuntime::load(&script, limits)
    }

    /// Evaluate one declared generator into a public commit-result record.
    /// The desktop host owns the tape and then passes this record to
    /// `HostSession::commit_generation`; the net crate deliberately does not
    /// depend on this Lua runtime.
    pub fn generate(
        &self,
        record_id: impl Into<String>,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
        limits: GeneratorLimits,
    ) -> Result<GenerationRecord, String> {
        let mut runtime = self.runtime_for(request, limits)?;
        let result = runtime.call(request, tape)?;
        let record = GenerationRecord {
            id: record_id.into(),
            request: request.clone(),
            entropy: result.entropy,
            proposal: result.value,
        };
        record
            .validate(limits.max_value_depth)
            .map_err(|error| format!("validate generation record: {error}"))?;
        Ok(record)
    }

    /// Load and run a fixture declared for one pack generator. The fixture's
    /// request must name that same fully-qualified generator, keeping fixtures
    /// from silently testing a script they do not describe.
    pub fn run_fixture(
        &self,
        generator: &str,
        fixture_path: &str,
        limits: GeneratorLimits,
    ) -> Result<(), String> {
        let entry = self
            .manifest
            .generator(generator)
            .ok_or_else(|| format!("generator is not declared by this pack: {generator}"))?;
        if !entry.fixtures.iter().any(|fixture| fixture == fixture_path) {
            return Err(format!(
                "fixture is not declared for generator {generator}: {fixture_path}"
            ));
        }
        let fixture_json = self.read_asset(fixture_path)?;
        let fixture: GeneratorFixture = serde_json::from_str(&fixture_json)
            .map_err(|error| format!("parse fixture {fixture_path}: {error}"))?;
        if fixture.request.generator != generator {
            return Err(format!(
                "fixture {fixture_path} names {}, expected {generator}",
                fixture.request.generator
            ));
        }
        let mut runtime = self.runtime_for(&fixture.request, limits)?;
        runtime.run_fixture(&fixture)
    }

    fn read_asset(&self, relative: &str) -> Result<String, String> {
        let path = self.root.join(relative);
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("open pack asset {relative}: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!("pack asset escapes root: {relative}"));
        }
        std::fs::read_to_string(&canonical)
            .map_err(|error| format!("read {}: {error}", canonical.display()))
    }
}

impl GeneratorRuntime {
    pub fn load(script: &str, limits: GeneratorLimits) -> Result<Self, String> {
        if limits.fuel <= 0 {
            return Err("generator fuel must be positive".to_owned());
        }
        let mut lua = Lua::core();
        let ex = lua
            .try_enter(|ctx| {
                let closure = Closure::load(ctx, Some("generator"), script.as_bytes())?;
                Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
            })
            .map_err(|e| format!("load generator script: {e}"))?;
        execute_bounded::<()>(&mut lua, &ex, limits.fuel)?;
        Ok(Self { lua, limits })
    }

    /// Execute a generator once with one host-owned entropy draw. Lua receives
    /// the draw as an `i64`, hence the high bit is cleared without changing the
    /// deterministic tape record.
    pub fn call(
        &mut self,
        request: &GeneratorRequest,
        tape: &mut EntropyTape,
    ) -> Result<GeneratorResult, String> {
        let args = serde_json::to_string(request)
            .map_err(|e| format!("serialize generator request: {e}"))?;
        let entropy = tape.draw();
        let lua_entropy = (entropy >> 1) as i64;
        let ex = self
            .lua
            .try_enter(move |ctx| {
                let request_table = generator_request_table(ctx, &request);
                let name = piccolo::String::from_slice(&ctx, b"call_gen");
                let Value::Function(function) = ctx.globals().get(ctx, name) else {
                    return Err("generator script does not define call_gen"
                        .into_value(ctx)
                        .into());
                };
                Ok(ctx.stash(Executor::start(
                    ctx,
                    function,
                    (args, lua_entropy, request_table),
                )))
            })
            .map_err(|e| format!("start generator: {e}"))?;
        let value = execute_bounded_gen_value(
            &mut self.lua,
            &ex,
            self.limits.fuel,
            self.limits.max_value_depth,
        )?;
        let output_bytes = serde_json::to_vec(&value)
            .map_err(|e| format!("serialize generated value for size check: {e}"))?;
        if output_bytes.len() > self.limits.max_output_bytes {
            return Err(format!(
                "generator output exceeds {} byte limit",
                self.limits.max_output_bytes
            ));
        }
        value
            .validate_depth(self.limits.max_value_depth)
            .map_err(|e| format!("generator returned invalid GenValue: {e}"))?;
        Ok(GeneratorResult { value, entropy })
    }

    /// Run one authored fixture without any campaign state. Both the proposal
    /// and entropy trace must match, so a changed number/order of random draws
    /// is visible even when it happens to produce the same proposal text.
    pub fn run_fixture(&mut self, fixture: &GeneratorFixture) -> Result<(), String> {
        let mut tape = EntropyTape::from_seed(fixture.seed);
        let result = self.call(&fixture.request, &mut tape)?;
        if result.value != fixture.expected {
            return Err(format!(
                "fixture {} returned a different proposal",
                fixture.name
            ));
        }
        if tape.draws != fixture.expected_draws {
            return Err(format!(
                "fixture {} recorded different entropy",
                fixture.name
            ));
        }
        Ok(())
    }
}

