fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();
    agentic_kernel::run()
}
