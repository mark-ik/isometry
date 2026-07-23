fn main() {
    let mut endpoint = isometry_graphshell::IsometryEndpoint::fixture();
    graphshell_stdio::serve_basic(
        &mut endpoint,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
    .expect("Isometry Graphshell endpoint failed");
}
