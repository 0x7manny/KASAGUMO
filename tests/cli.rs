use std::process::{Command, Output};

fn kgo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kgo"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_liste_les_commandes() {
    let out = kgo(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for cmd in ["node", "nodes", "run", "ps", "stop", "chunk"] {
        assert!(stdout.contains(cmd), "{cmd} absent de l'aide");
    }
}

#[test]
fn commande_non_implementee_echoue() {
    let out = kgo(&["run", "--cpu", "2", "--memory", "4GB", "nginx:latest"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("4096 Mo"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("pas encore implémenté"));
}

#[test]
fn memoire_invalide_est_refusee() {
    let out = kgo(&["run", "--memory", "4TB", "nginx"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unité inconnue"));
}

#[test]
fn chunk_fichier_inexistant() {
    let out = kgo(&["chunk", "/nope/introuvable.png"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("impossible de découper"));
}

#[test]
fn chunk_sur_un_vrai_fichier() {
    let path = std::env::temp_dir().join(format!("kgo-test-{}.bin", std::process::id()));
    std::fs::write(&path, vec![7u8; 3 * 1024 * 1024]).unwrap();
    let out = kgo(&["chunk", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("intégrité : OK"));
}

#[test]
fn ps_sans_daemon_explique_quoi_faire() {
    let dir = std::env::temp_dir().join(format!("kgo-nodaemon-{}", std::process::id()));
    let out = kgo(&["ps", "--data-dir", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("kgo node start"));
}

#[test]
fn ps_parle_au_daemon() {
    let dir = std::env::temp_dir().join(format!("kgo-daemon-{}", std::process::id()));
    let dir_str = dir.to_str().unwrap();

    let mut daemon = Command::new(env!("CARGO_BIN_EXE_kgo"))
        .args(["node", "start", "--data-dir", dir_str])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let socket = dir.join("kgo.sock");
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let out = kgo(&["ps", "--data-dir", dir_str]);
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    std::fs::remove_dir_all(&dir).ok();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("aucun workload"));
}
