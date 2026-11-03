use super::*;

fn pr_artifact_run(id: u64, conclusion: &str) -> Value {
    json!({"id": id, "workflow_id": 10, "run_number": id, "event": "pull_request",
        "status": "completed", "conclusion": conclusion, "pull_requests": [{"number": 7}],
        "created_at": "2026-10-29T12:00:00Z", "head_sha": "abc",
        "head_branch": "feature", "head_repository": {"id": 42, "full_name": "example/project"}})
}

fn pr_build_fixture(sandbox: &Sandbox, include_image: bool) {
    sandbox.response("GET", "repos/example/project/git/trees/tree?recursive=1", 0, json!({"truncated":false,"tree":[{"path":"Cargo.toml","mode":"100644","sha":"source","type":"blob"}]}));
    let mut artifacts = vec![
        json!({"id":1,"name":"pr-source-b5d24126dec10091c2cb3e66b4299792d3f28e079b639dde26f70304fc80ba46","expired":false,"workflow_run":{"id":1}}),
        json!({"id":2,"name":"package","expired":false,"workflow_run":{"id":1}}),
    ];
    if include_image {
        artifacts.push(json!({"id":3,"name":"image","expired":false,"workflow_run":{"id":1}}));
    }

    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/git/commits/abc",
        0,
        json!({"tree":{"sha":"tree"}}),
    );
    fs::write(
        sandbox.root().join("event"),
        r#"{"pull_request":{"number":7}}"#,
    )
    .unwrap();
}

#[test]
fn unchanged_build_reuses_a_complete_package_and_source_marker() {
    let sandbox = Sandbox::new();
    pr_build_fixture(&sandbox, true);
    successful(
        sandbox
            .command("pr-artifact-build")
            .env("GITHUB_EVENT_PATH", sandbox.root().join("event"))
            .env("GITHUB_RUN_ID", "2")
            .env("GITHUB_SHA", "abc")
            .env("SOURCE_PATHS", r#"["Cargo.toml"]"#)
            .env("REQUIRED_ARTIFACTS", r#"["package","image"]"#)
            .output()
            .unwrap(),
    );
    let output = fs::read_to_string(sandbox.root().join("output")).unwrap();
    assert!(output.contains("build=false"));
    assert!(output.contains("run-id=1"));
    assert!(output.contains(r#"artifacts=["package","image","pr-source-b5d24126dec10091c2cb3e66b4299792d3f28e079b639dde26f70304fc80ba46"]"#));
    assert!(sandbox.calls().iter().all(|call| call.method == "GET"));
}

#[test]
fn unchanged_build_rebuilds_when_any_required_output_is_missing() {
    let sandbox = Sandbox::new();
    pr_build_fixture(&sandbox, false);
    successful(
        sandbox
            .command("pr-artifact-build")
            .env("GITHUB_EVENT_PATH", sandbox.root().join("event"))
            .env("GITHUB_RUN_ID", "2")
            .env("GITHUB_SHA", "abc")
            .env("SOURCE_PATHS", r#"["Cargo.toml"]"#)
            .env("REQUIRED_ARTIFACTS", r#"["package","image"]"#)
            .output()
            .unwrap(),
    );
    let output = fs::read_to_string(sandbox.root().join("output")).unwrap();
    assert!(output.contains("build=true"));
    assert!(output.contains("run-id=0"));
}

#[test]
fn cleanup_deletes_older_runs_but_preserves_outputs_from_partial_reruns() {
    let sandbox = Sandbox::new();
    let artifacts: Vec<_> = [(1, 1), (2, 2), (3, 2)]
        .into_iter()
        .map(
            |(id, run)| json!({"id":id,"name":"package","expired":false,"workflow_run":{"id":run}}),
        )
        .collect();
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "DELETE",
        "repos/example/project/actions/artifacts/1",
        0,
        Value::Null,
    );
    successful(sandbox.command("pr-artifact-cleanup").output().unwrap());
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .map(|call| call.endpoint)
        .collect();
    assert_eq!(deleted, ["repos/example/project/actions/artifacts/1"]);
}

#[test]
fn cleanup_paginates_and_continues_when_an_artifact_was_already_deleted() {
    let sandbox = Sandbox::new();
    let artifacts: Vec<_> = (1..=100)
        .map(|id| json!({"id":id,"name":"package","expired":false,"workflow_run":{"id":1}}))
        .collect();
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=2",
        0,
        json!({"artifacts":[{"id":101,"name":"package","expired":false,"workflow_run":{"id":2}}]}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/1",
        0,
        pr_artifact_run(1, "success"),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    for id in 1..=100 {
        let endpoint = format!("repos/example/project/actions/artifacts/{id}");
        sandbox.response(
            "DELETE",
            &endpoint,
            0,
            if id == 1 {
                json!({"status":"404"})
            } else {
                Value::Null
            },
        );
        if id == 1 {
            let key: String = endpoint.bytes().map(|byte| format!("{byte:02x}")).collect();
            fs::write(sandbox.root().join(format!("DELETE-{key}.exit")), "1").unwrap();
        }
    }

    successful(sandbox.command("pr-artifact-cleanup").output().unwrap());
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .collect();
    assert_eq!(deleted.len(), 100);
    assert!(deleted.iter().all(|call| !call.endpoint.ends_with("/101")));
}

#[test]
fn cleanup_removes_consumed_inputs_only_after_a_successful_final_upload() {
    let sandbox = Sandbox::new();
    let artifacts = json!([{"id":1,"name":"input-binary","expired":false,"workflow_run":{"id":2}},
        {"id":2,"name":"package","expired":false,"workflow_run":{"id":2}}]);
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    sandbox.response(
        "GET",
        "repos/example/project/actions/runs/2",
        0,
        pr_artifact_run(2, "success"),
    );
    sandbox.response(
        "DELETE",
        "repos/example/project/actions/artifacts/1",
        0,
        Value::Null,
    );
    successful(
        sandbox
            .command("pr-artifact-cleanup")
            .env("TRANSIENT_PREFIX", "input-")
            .output()
            .unwrap(),
    );
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .map(|call| call.endpoint)
        .collect();
    assert_eq!(deleted, ["repos/example/project/actions/artifacts/1"]);
}

#[test]
fn reuse_rejects_a_marker_forged_by_different_producer_inputs() {
    let sandbox = Sandbox::new();
    pr_build_fixture(&sandbox, true);
    let mut producer = pr_artifact_run(1, "success");
    producer["head_sha"] = json!("bad");
    sandbox.response("GET", "repos/example/project/actions/runs/1", 0, producer);
    sandbox.response(
        "GET",
        "repos/example/project/git/commits/bad",
        0,
        json!({"tree":{"sha":"bad-tree"}}),
    );
    sandbox.response("GET", "repos/example/project/git/trees/bad-tree?recursive=1", 0,
        json!({"truncated":false,"tree":[{"path":"Cargo.toml","mode":"100644","sha":"tainted","type":"blob"}]}));
    successful(
        sandbox
            .command("pr-artifact-build")
            .env("GITHUB_EVENT_PATH", sandbox.root().join("event"))
            .env("GITHUB_RUN_ID", "2")
            .env("GITHUB_SHA", "abc")
            .env("SOURCE_PATHS", r#"["Cargo.toml"]"#)
            .env("REQUIRED_ARTIFACTS", r#"["package","image"]"#)
            .output()
            .unwrap(),
    );
    assert!(
        fs::read_to_string(sandbox.root().join("output"))
            .unwrap()
            .contains("build=true")
    );
}

#[test]
fn cleanup_resolves_the_target_pr_from_its_branch_after_a_stacked_merge() {
    let sandbox = Sandbox::new();
    let artifacts: Vec<_> = [1, 2]
        .into_iter()
        .map(|id| json!({"id":id,"name":"package","expired":false,"workflow_run":{"id":id}}))
        .collect();
    sandbox.response(
        "GET",
        "repos/example/project/actions/artifacts?per_page=100&page=1",
        0,
        json!({"artifacts":artifacts}),
    );
    for id in [1, 2] {
        let mut run = pr_artifact_run(id, "success");
        run["pull_requests"] = json!([]);
        sandbox.response(
            "GET",
            &format!("repos/example/project/actions/runs/{id}"),
            0,
            run,
        );
    }

    sandbox.response("GET", "repos/example/project/pulls?state=all&head=example%3Afeature&per_page=100", 0,
        json!([[{"number":7,"created_at":"2026-10-28T12:00:00Z","closed_at":"2026-10-30T12:00:00Z",
            "head":{"ref":"feature","repo":{"id":42}},"base":{"repo":{"full_name":"example/project"}}}]]));
    sandbox.response(
        "DELETE",
        "repos/example/project/actions/artifacts/1",
        0,
        Value::Null,
    );
    successful(sandbox.command("pr-artifact-cleanup").output().unwrap());
    let deleted: Vec<_> = sandbox
        .calls()
        .into_iter()
        .filter(|call| call.method == "DELETE")
        .map(|call| call.endpoint)
        .collect();
    assert_eq!(deleted, ["repos/example/project/actions/artifacts/1"]);
}
