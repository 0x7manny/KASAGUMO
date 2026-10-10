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
fn run_sans_daemon_explique_quoi_faire() {
    let dir = std::env::temp_dir().join(format!("kgo-norun-{}", std::process::id()));
    let out = kgo(&["run", "--data-dir", dir.to_str().unwrap(), "nginx"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("kgo node start"));
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

/// Crée un cluster dans `dir`, qui garde la clé servant à y admettre des nœuds.
fn new_cluster(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::remove_dir_all(dir).ok();
    let out = kgo(&["--data-dir", dir.to_str().unwrap(), "cluster", "init"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    dir.to_path_buf()
}

/// Le cluster commun aux nœuds de ces tests.
fn test_cluster() -> &'static std::path::Path {
    static CLUSTER: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    CLUSTER.get_or_init(|| new_cluster(&std::env::temp_dir().join(format!("kgo-cluster-{}", std::process::id())))).as_path()
}

/// Admet le nœud de `dir` dans le cluster de `cluster` (sans effet s'il en fait déjà partie).
fn enroll(dir: &std::path::Path, cluster: &std::path::Path) {
    if dir.join("cluster.pub").exists() {
        return;
    }
    let run = |data_dir: &std::path::Path, args: &[&str]| {
        let out = kgo(&[args, &["--data-dir", data_dir.to_str().unwrap()]].concat());
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let public_key = run(dir, &["node", "id"]);
    let certificate = run(cluster, &["cluster", "admit", &public_key]);
    run(dir, &["node", "enroll", &certificate]);
}

struct Daemon {
    dir: std::path::PathBuf,
    child: std::process::Child,
    port: u16,
    _stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl Daemon {
    fn start(dir: &std::path::Path) -> Self {
        Self::start_with_docker(dir, true, test_cluster(), 0)
    }

    /// Redémarre un nœud sur le port qu'il avait.
    fn start_on(dir: &std::path::Path, port: u16) -> Self {
        Self::start_with_docker(dir, true, test_cluster(), port)
    }

    /// `containers_alive` : ce que répond le docker factice à `docker inspect`.
    fn start_with_docker(dir: &std::path::Path, containers_alive: bool, cluster: &std::path::Path, port: u16) -> Self {
        use std::io::BufRead;
        use std::os::unix::fs::PermissionsExt;

        std::fs::create_dir_all(dir).unwrap();
        enroll(dir, cluster);
        let docker = dir.join("fake-docker.sh");
        let script = format!("#!/bin/sh\n[ \"$1\" = inspect ] && echo {containers_alive}\n[ \"$1\" = logs ] && echo hello-from-$4\nexit 0\n");
        std::fs::write(&docker, script).unwrap();
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_kgo"))
            .args(["node", "start", "--port", &port.to_string(), "--heartbeat-ms", "100", "--data-dir", dir.to_str().unwrap()])
            .env("KGO_DOCKER", &docker) // docker factice : toutes les commandes réussissent
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut port = None;
        let mut line = String::new();
        while port.is_none() && stdout.read_line(&mut line).unwrap() > 0 {
            port = line.trim().strip_prefix("tcp : 0.0.0.0:").map(|p| p.parse().unwrap());
            line.clear();
        }
        let port = port.expect("le daemon n'a pas annoncé son port");
        Self { dir: dir.to_path_buf(), child, port, _stdout: stdout }
    }

    fn kgo(&self, args: &[&str]) -> Output {
        let mut full = args.to_vec();
        full.extend(["--data-dir", self.dir.to_str().unwrap()]);
        kgo(&full)
    }

    fn stop(self) {}
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Attend que `kgo ps` affiche `expected` (le démarrage est asynchrone).
fn wait_for_ps(daemon: &Daemon, expected: &str) -> String {
    for _ in 0..100 {
        let ps = text(&daemon.kgo(&["ps"]).stdout);
        if ps.contains(expected) {
            return ps;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("`kgo ps` n'a jamais affiché « {expected} »");
}

#[test]
fn ps_parle_au_daemon() {
    let dir = std::env::temp_dir().join(format!("kgo-daemon-{}", std::process::id()));
    let daemon = Daemon::start(&dir);
    let out = daemon.kgo(&["ps"]);
    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();

    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("aucun workload"));
}

#[test]
fn cycle_de_vie_et_persistance() {
    let dir = std::env::temp_dir().join(format!("kgo-cycle-{}", std::process::id()));
    let daemon = Daemon::start(&dir);

    let out = daemon.kgo(&["run", "--cpu", "2", "--memory", "4GB", "nginx:latest"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("pending"));
    let id = stdout.split_whitespace().next().unwrap().to_string();

    let ps = wait_for_ps(&daemon, "running");
    assert!(ps.contains(&id) && ps.contains("nginx:latest") && ps.contains("4096MB"));

    // le daemon est tué brutalement puis relancé : l'état doit survivre
    daemon.stop();
    let daemon = Daemon::start(&dir);
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains(&id));

    let out = daemon.kgo(&["stop", &id]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains("aucun workload"));
    assert!(text(&daemon.kgo(&["ps", "--all"]).stdout).contains("stopped"));

    let out = daemon.kgo(&["stop", "inconnu"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("introuvable"));

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn le_port_tcp_est_chiffre_et_reserve_aux_membres() {
    use std::io::{Read, Write};

    let base = std::env::temp_dir().join(format!("kgo-tls-{}", std::process::id()));
    let a = Daemon::start(&base.join("a"));
    let intrus = Daemon::start_with_docker(&base.join("intrus"), true, &new_cluster(&base.join("autre-cluster")), 0);

    // en clair, le nœud ne répond rien d'exploitable
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", a.port)).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    writeln!(stream, r#""Info""#).unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).ok();
    assert!(!String::from_utf8_lossy(&reply).contains("cpus"));

    // un nœud d'un autre cluster est rejeté, dans les deux sens
    let on = format!("127.0.0.1:{}", a.port);
    let out = intrus.kgo(&["--on", &on, "ps"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("membre du cluster"), "{}", text(&out.stderr));
    let out = a.kgo(&["--on", &format!("127.0.0.1:{}", intrus.port), "ps"]);
    assert!(text(&out.stderr).contains("membre du cluster"), "{}", text(&out.stderr));

    a.stop();
    intrus.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn join_et_nodes() {
    let base = std::env::temp_dir().join(format!("kgo-nodes-{}", std::process::id()));
    let (dir_a, dir_b) = (base.join("a"), base.join("b"));
    let a = Daemon::start(&dir_a);
    let b = Daemon::start(&dir_b);

    let addr_b = format!("127.0.0.1:{}", b.port);
    let out = a.kgo(&["node", "join", &addr_b]);
    assert!(out.status.success(), "{}", text(&out.stderr));

    let listing = text(&a.kgo(&["nodes"]).stdout);
    assert!(listing.contains(&addr_b) && listing.contains("up"), "{listing}");
    assert_eq!(listing.matches(" up").count(), 2, "{listing}");

    // un pair injoignable est signalé mais n'empêche pas la liste
    b.stop();
    let listing = text(&a.kgo(&["nodes"]).stdout);
    assert!(listing.contains("injoignable"), "{listing}");
    assert!(listing.contains("localhost"), "{listing}");

    // on ne retient pas une adresse qui ne répond pas
    let out = a.kgo(&["node", "join", &addr_b]);
    assert_eq!(out.status.code(), Some(1));

    a.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn reconciliation_au_redemarrage() {
    let dir = std::env::temp_dir().join(format!("kgo-reconcile-{}", std::process::id()));
    let daemon = Daemon::start(&dir);
    let out = daemon.kgo(&["run", "nginx"]);
    let id = text(&out.stdout).split_whitespace().next().unwrap().to_string();
    wait_for_ps(&daemon, "running");

    // le daemon redémarre alors que le conteneur a disparu : le workload devient failed
    daemon.stop();
    let daemon = Daemon::start_with_docker(&dir, false, test_cluster(), 0);
    assert!(text(&daemon.kgo(&["ps"]).stdout).contains("aucun workload"));
    let all = text(&daemon.kgo(&["ps", "--all"]).stdout);
    assert!(all.contains(&id) && all.contains("failed"), "{all}");

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn run_ps_stop_sur_un_pair() {
    let base = std::env::temp_dir().join(format!("kgo-forward-{}", std::process::id()));
    let (a, b) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")));
    let on = format!("127.0.0.1:{}", b.port);

    let out = a.kgo(&["--on", &on, "run", "nginx"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let id = text(&out.stdout).split_whitespace().next().unwrap().to_string();

    // le workload vit sur b, pas sur a
    assert!(text(&a.kgo(&["ps"]).stdout).contains("aucun workload"));
    assert!(text(&b.kgo(&["ps"]).stdout).contains(&id));
    assert!(text(&a.kgo(&["--on", &on, "ps"]).stdout).contains(&id));

    let out = a.kgo(&["--on", &on, "stop", &id]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&b.kgo(&["ps"]).stdout).contains("aucun workload"));

    a.stop();
    b.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn run_choisit_le_noeud_le_plus_libre() {
    let base = std::env::temp_dir().join(format!("kgo-schedule-{}", std::process::id()));
    let (a, b) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")));
    let addr_b = format!("127.0.0.1:{}", b.port);
    assert!(a.kgo(&["node", "join", &addr_b]).status.success());

    // a est plein : le workload suivant part sur b
    let cpus = std::thread::available_parallelism().unwrap().get().to_string();
    let out = text(&a.kgo(&["run", "--cpu", &cpus, "nginx"]).stdout);
    assert!(out.contains("localhost"), "{out}");
    let out = text(&a.kgo(&["run", "nginx"]).stdout);
    assert!(out.contains(&addr_b), "{out}");
    assert!(text(&b.kgo(&["ps"]).stdout).contains("nginx"));

    // plus aucune place nulle part
    let out = a.kgo(&["run", "--cpu", &cpus, "nginx"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("CPU et"), "{}", text(&out.stderr));

    a.stop();
    b.stop();
    std::fs::remove_dir_all(&base).ok();
}

/// Attend que `check` soit vrai, au plus ~5 s.
fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("{what}");
}

#[test]
fn les_pairs_se_decouvrent_par_gossip() {
    let base = std::env::temp_dir().join(format!("kgo-gossip-{}", std::process::id()));
    let (a, b, c) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")), Daemon::start(&base.join("c")));
    let addr = |d: &Daemon| format!("127.0.0.1:{}", d.port);

    // a et c ne connaissent que b ; b apprend a et c par leurs battements, puis les leur transmet
    assert!(a.kgo(&["node", "join", &addr(&b)]).status.success());
    assert!(c.kgo(&["node", "join", &addr(&b)]).status.success());
    wait_until("a n'a pas découvert c", || {
        let nodes = text(&a.kgo(&["nodes"]).stdout);
        nodes.contains(&addr(&c)) && nodes.matches(" up").count() == 3
    });

    a.stop();
    b.stop();
    c.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn un_pair_tombe_ses_workloads_sont_replaces() {
    let base = std::env::temp_dir().join(format!("kgo-failover-{}", std::process::id()));
    let mut nodes = vec![Daemon::start(&base.join("a")), Daemon::start(&base.join("b")), Daemon::start(&base.join("c"))];
    let addr = |d: &Daemon| format!("127.0.0.1:{}", d.port);
    for peer in [1, 2] {
        assert!(nodes[0].kgo(&["node", "join", &addr(&nodes[peer])]).status.success());
    }

    // a est plein : le workload part sur b ou c, que a surveille
    let cpus = std::thread::available_parallelism().unwrap().get().to_string();
    assert!(nodes[0].kgo(&["run", "--cpu", &cpus, "redis"]).status.success());
    let placed = text(&nodes[0].kgo(&["run", "nginx"]).stdout);
    let victim = nodes.iter().position(|d| placed.contains(&addr(d))).unwrap();
    assert_ne!(victim, 0, "{placed}");

    // le pair qui l'héberge tombe : il est relancé sur l'autre
    let survivor = 3 - victim;
    let (port, dir) = (nodes[victim].port, nodes[victim].dir.clone());
    nodes.remove(victim).stop();
    let survivor = if survivor > victim { survivor - 1 } else { survivor };
    wait_until("nginx n'a pas été relancé", || text(&nodes[survivor].kgo(&["ps"]).stdout).contains("nginx"));

    // le pair revient avec son ancien workload : a l'arrête, il n'y a plus de doublon
    let back = Daemon::start_on(&dir, port);
    wait_until("le doublon n'a pas été arrêté", || text(&back.kgo(&["ps", "--all"]).stdout).contains("stopped"));
    assert!(text(&nodes[survivor].kgo(&["ps"]).stdout).contains("nginx"));

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn logs_suivent_le_workload_sur_son_noeud() {
    let base = std::env::temp_dir().join(format!("kgo-logs-{}", std::process::id()));
    let (a, b) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")));
    assert!(a.kgo(&["node", "join", &format!("127.0.0.1:{}", b.port)]).status.success());

    // a est plein : le workload part sur b, mais `kgo logs` s'adresse à a
    let cpus = std::thread::available_parallelism().unwrap().get().to_string();
    assert!(a.kgo(&["run", "--cpu", &cpus, "redis"]).status.success());
    let placed = text(&a.kgo(&["run", "nginx"]).stdout);
    let id = placed.split_whitespace().next().unwrap();
    assert!(text(&a.kgo(&["logs", id]).stdout).contains(&format!("hello-from-kgo-{id}")));

    let out = a.kgo(&["logs", "inconnu"]);
    assert_eq!(out.status.code(), Some(1));

    a.stop();
    b.stop();
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn run_refuse_plus_de_memoire_que_le_nœud_n_en_a() {
    let dir = std::env::temp_dir().join(format!("kgo-memory-{}", std::process::id()));
    let daemon = Daemon::start(&dir);

    let out = daemon.kgo(&["run", "--memory", "100000GB", "nginx"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("Mo libres"), "{}", text(&out.stderr));
    assert!(text(&daemon.kgo(&["nodes"]).stdout).contains("Mo"));

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn put_et_get_repartis_sur_le_cluster() {
    let base = std::env::temp_dir().join(format!("kgo-blobs-{}", std::process::id()));
    let (a, b) = (Daemon::start(&base.join("a")), Daemon::start(&base.join("b")));
    assert!(a.kgo(&["node", "join", &format!("127.0.0.1:{}", b.port)]).status.success());

    // 2,5 Mio : trois blocs de données plus le manifeste
    std::fs::create_dir_all(&base).unwrap();
    let (input, output) = (base.join("in.bin"), base.join("out.bin"));
    let content: Vec<u8> = (0..2_500_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
    std::fs::write(&input, &content).unwrap();

    let out = a.kgo(&["put", input.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let id = text(&out.stdout).trim().to_string();
    assert!(text(&out.stderr).contains("2 copies minimum"), "{}", text(&out.stderr));

    // depuis l'autre nœud, puis après la chute du premier : les copies suffisent
    for (reader, path) in [(&b, "b1.bin"), (&b, "b2.bin")] {
        let path = base.join(path);
        let out = reader.kgo(&["get", &id, path.to_str().unwrap()]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(std::fs::read(&path).unwrap(), content);
        if path.ends_with("b1.bin") {
            // a disparaît : b garde tous les blocs
            std::fs::remove_file(&path).unwrap();
        }
    }
    a.stop();
    let out = b.kgo(&["get", &id, output.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(std::fs::read(&output).unwrap(), content);

    // identifiant inconnu ou invalide
    for bad in ["0".repeat(64), "../../etc/passwd".to_string()] {
        let out = b.kgo(&["get", &bad, output.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(1));
    }

    b.stop();
    std::fs::remove_dir_all(&base).ok();
}

/// Les blocs rangés sur un nœud (un fichier par bloc dans `<dossier>/blobs`).
fn blobs_of(daemon: &Daemon) -> std::collections::BTreeSet<String> {
    let entries = std::fs::read_dir(daemon.dir.join("blobs")).unwrap();
    entries.map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
}

#[test]
fn les_blocs_sont_re_repliques_quand_un_noeud_tombe() {
    let base = std::env::temp_dir().join(format!("kgo-repair-{}", std::process::id()));
    let mut nodes = vec![Daemon::start(&base.join("a")), Daemon::start(&base.join("b")), Daemon::start(&base.join("c"))];
    let addr = |d: &Daemon| format!("127.0.0.1:{}", d.port);
    for peer in [1, 2] {
        assert!(nodes[0].kgo(&["node", "join", &addr(&nodes[peer])]).status.success());
    }
    wait_until("le cluster ne se connaît pas", || {
        nodes.iter().all(|n| text(&n.kgo(&["nodes"]).stdout).matches(" up").count() == 3)
    });

    std::fs::create_dir_all(&base).unwrap();
    let input = base.join("in.bin");
    std::fs::write(&input, vec![7u8; 2_500_000]).unwrap();
    let out = nodes[0].kgo(&["put", input.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let all: std::collections::BTreeSet<String> = nodes.iter().flat_map(blobs_of).collect();

    // c tombe : a et b, qui se partageaient les copies, doivent tout détenir
    nodes.remove(2).stop();
    wait_until("les copies n'ont pas été refaites", || nodes.iter().all(|n| blobs_of(n) == all));

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn put_d_un_fichier_introuvable_echoue_proprement() {
    let dir = std::env::temp_dir().join(format!("kgo-put-missing-{}", std::process::id()));
    let daemon = Daemon::start(&dir);

    let out = daemon.kgo(&["put", "/inexistant/fichier.bin"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("impossible de lire"), "{}", text(&out.stderr));
    assert!(text(&out.stdout).is_empty());

    // un get qui échoue ne laisse pas de fichier
    let target = dir.join("sortie.bin");
    assert_eq!(daemon.kgo(&["get", &"0".repeat(64), target.to_str().unwrap()]).status.code(), Some(1));
    assert!(!target.exists());

    daemon.stop();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn les_workloads_d_un_noeud_tombe_sont_repris_une_seule_fois() {
    let base = std::env::temp_dir().join(format!("kgo-takeover-{}", std::process::id()));
    let mut nodes = vec![Daemon::start(&base.join("a")), Daemon::start(&base.join("b")), Daemon::start(&base.join("c"))];
    let addr = |d: &Daemon| format!("127.0.0.1:{}", d.port);
    for peer in [1, 2] {
        assert!(nodes[0].kgo(&["node", "join", &addr(&nodes[peer])]).status.success());
    }
    wait_until("le cluster ne se connaît pas", || {
        nodes.iter().all(|n| text(&n.kgo(&["nodes"]).stdout).matches(" up").count() == 3)
    });

    // le workload tourne sur a, qui l'a lancé lui-même ; b et c l'apprennent par ses battements
    let placed = text(&nodes[0].kgo(&["run", "nginx"]).stdout);
    assert!(placed.contains("localhost"), "{placed}");
    std::thread::sleep(std::time::Duration::from_millis(700));

    // a tombe avec son workload : un seul des deux survivants le reprend
    nodes.remove(0).stop();
    let running = |nodes: &[Daemon]| nodes.iter().filter(|n| text(&n.kgo(&["ps"]).stdout).contains("nginx")).count();
    wait_until("le workload n'a pas été repris", || running(&nodes) >= 1);
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert_eq!(running(&nodes), 1);

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn les_commandes_de_cluster_sont_controlees() {
    let base = std::env::temp_dir().join(format!("kgo-cluster-cmds-{}", std::process::id()));
    std::fs::remove_dir_all(&base).ok();
    let (a, b) = (base.join("a"), base.join("b"));
    let dir = |d: &std::path::Path| d.to_str().unwrap().to_string();

    // sans cluster, un nœud ne démarre pas
    let out = kgo(&["node", "start", "--data-dir", &dir(&a)]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("aucun cluster"), "{}", text(&out.stderr));

    // un cluster ne se crée qu'une fois, et seul son créateur admet des nœuds
    assert!(kgo(&["cluster", "init", "--data-dir", &dir(&a)]).status.success());
    assert_eq!(kgo(&["cluster", "init", "--data-dir", &dir(&a)]).status.code(), Some(1));
    let public_key = text(&kgo(&["node", "id", "--data-dir", &dir(&b)]).stdout);
    assert_eq!(kgo(&["cluster", "admit", public_key.trim(), "--data-dir", &dir(&b)]).status.code(), Some(1));

    // un certificat n'est valable que pour le nœud auquel il est destiné
    let certificate = text(&kgo(&["cluster", "admit", public_key.trim(), "--data-dir", &dir(&a)]).stdout);
    let other = base.join("c");
    assert_eq!(kgo(&["node", "enroll", certificate.trim(), "--data-dir", &dir(&other)]).status.code(), Some(1));
    assert!(kgo(&["node", "enroll", certificate.trim(), "--data-dir", &dir(&b)]).status.success());

    std::fs::remove_dir_all(&base).ok();
}
