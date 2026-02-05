mod app;
mod assets;
mod ipc;
mod state;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
