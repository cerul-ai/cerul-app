pub fn workspace_ready() -> bool {
    cerul_api::crate_ready()
        && cerul_embed::crate_ready()
        && cerul_pipeline::crate_ready()
        && cerul_search::crate_ready()
        && cerul_sources::crate_ready()
        && cerul_storage::crate_ready()
}
