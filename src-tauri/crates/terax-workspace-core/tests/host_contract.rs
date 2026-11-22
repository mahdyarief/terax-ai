use terax_workspace_core::contract;
use terax_workspace_core::env::WorkspaceEnv;
use terax_workspace_core::host::HostWorkspace;

#[test]
fn the_host_backend_meets_the_workspace_contract() {
    let dir = tempfile::tempdir().unwrap();
    let ws = HostWorkspace::new(WorkspaceEnv::Local);
    contract::run(&ws, dir.path().to_str().unwrap());
}
