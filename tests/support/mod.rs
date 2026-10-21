use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            fs::{PermissionsExt, symlink},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn fixture_process(name: &str, args: &[String]) -> Result<i32> {
    if name != "herdr" {
        let argv: Vec<_> = std::iter::once(name.to_owned())
            .chain(args.iter().cloned())
            .collect();
        fs::write(env::var("HERD_AGENT_LOG")?, serde_json::to_vec(&argv)?)?;
        return Ok(env::var("HERD_AGENT_EXIT")
            .unwrap_or_else(|_| "0".into())
            .parse()?);
    }
    if args == ["--version"] {
        println!(
            "herdr {}",
            env::var("HERD_TEST_VERSION").unwrap_or_else(|_| "0.7.3".into())
        );
        return Ok(0);
    }
    let log = PathBuf::from(env::var("HERD_TEST_LOG")?);
    let count = fs::read_to_string(&log).unwrap_or_default().lines().count();
    let mut line = serde_json::to_vec(args)?;
    line.push(b'\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)?
        .write_all(&line)?;
    if env::var_os("HERD_TEST_OFFLINE").is_some() {
        eprintln!("socket unavailable");
        return Ok(1);
    }
    let operation = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let error = |message| println!("{}", json!({"error": {"message": message}}));
    if env::var("HERD_TEST_ERROR").as_deref() == Ok(&operation) {
        error("requested operation failed");
        return Ok(0);
    }
    if env::var("HERD_TEST_EMPTY").as_deref() == Ok(&operation) {
        return Ok(0);
    }
    let mut result = match operation.as_str() {
        "tab create" => json!({"root_pane": {"pane_id": "boss-pane"}}),
        "pane split" => json!({"pane": {"pane_id": format!("worker-{count}")}}),
        "agent list" => json!({"agents": read_json(env::var("HERD_TEST_AGENTS")?) }),
        "pane get" => {
            let panes = read_json(env::var("HERD_TEST_PANES")?);
            if panes.get(&args[2]).is_none() {
                error("pane not found");
                return Ok(0);
            }
            json!({"pane": panes[&args[2]]})
        }
        "agent rename" => {
            if env::var_os("HERD_TEST_RENAME_ERROR").is_some() {
                error("rename failed");
                return Ok(0);
            }
            let path = env::var("HERD_TEST_AGENTS")?;
            let mut agents = read_json(&path).as_array().unwrap().clone();
            if let Some(current) = agents.iter_mut().find(|agent| agent["pane_id"] == args[2]) {
                current["name"] = json!(args[3]);
            } else {
                agents.push(json!({"pane_id": args[2], "name": args[3]}));
            }
            fs::write(path, serde_json::to_vec(&agents)?)?;
            json!({})
        }
        "tab list" => json!({"tabs": []}),
        _ => json!({}),
    };
    if env::var("HERD_TEST_INVALID_PANE").as_deref() == Ok(&operation) {
        for value in result.as_object_mut().unwrap().values_mut() {
            if value.get("pane_id").is_some() {
                value["pane_id"] = json!("");
            }
        }
    }
    println!("{}", json!({"result": result}));
    Ok(0)
}

pub fn read_json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

pub fn write_json(path: impl AsRef<Path>, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

pub fn status(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        stdout(output),
        stderr(output)
    );
}
pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}
pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn output(mut command: Command, input: &str) -> Output {
    let mut child = Running(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let mut out = child.0.stdout.take().unwrap();
    let mut err = child.0.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        out.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        err.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "The test command timed out.");
        thread::sleep(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

pub struct Fixture {
    pub root: PathBuf,
    pub package: PathBuf,
    pub bin: PathBuf,
    pub home: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Fixture {
    pub fn new() -> Self {
        static NUMBER: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "shepherd-test-{}-{}-{unique}",
            std::process::id(),
            NUMBER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = root.canonicalize().unwrap();
        let package = root.join("package");
        copy_package(&package);
        let bin = root.join("dependencies");
        let home = root.join("home");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&home).unwrap();
        for name in ["herdr", "claude", "codex"] {
            symlink(env::current_exe().unwrap(), bin.join(name)).unwrap();
        }
        write_json(root.join("agents.json"), &json!([]));
        write_json(root.join("panes.json"), &json!({}));
        let vars = [
            ("HOME", home.clone()),
            ("PATH", bin.clone()),
            ("TMPDIR", root.clone()),
            ("HERD_TEST_LOG", root.join("calls.jsonl")),
            ("HERD_TEST_AGENTS", root.join("agents.json")),
            ("HERD_TEST_PANES", root.join("panes.json")),
            ("HERD_AGENT_LOG", root.join("agent.json")),
        ];
        let env = vars
            .into_iter()
            .map(|(key, value)| (key.into(), value.to_str().unwrap().into()))
            .collect();
        Self {
            root,
            package,
            bin,
            home,
            env,
        }
    }

    pub fn command(&self, executable: impl AsRef<Path>, args: &[&str]) -> Command {
        let mut command = Command::new(executable.as_ref());
        command
            .args(args)
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.root);
        command
    }
    pub fn run(&self, args: &[&str]) -> Output {
        self.input(args, "")
    }
    pub fn input(&self, args: &[&str], input: &str) -> Output {
        output(
            self.command(self.package.join("bin/herdr-shepherd"), args),
            input,
        )
    }
    pub fn install(&self, args: &[&str]) -> Output {
        let mut arguments = vec!["--install"];
        arguments.extend_from_slice(args);
        self.run(&arguments)
    }
    pub fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.root.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    pub fn clear_calls(&self) {
        let _ = fs::remove_file(self.root.join("calls.jsonl"));
    }
    pub fn launches(&self) -> Vec<Vec<String>> {
        self.calls()
            .iter()
            .filter(|call| call[..2] == ["pane", "send-text"])
            .map(|call| {
                status(&output(self.command("/bin/sh", &["-c", &call[3]]), ""), 0);
                let mut argv: Vec<String> =
                    serde_json::from_value(read_json(self.root.join("agent.json"))).unwrap();
                argv[0] = Path::new(&argv[0])
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into();
                argv
            })
            .collect()
    }
    pub fn mutations(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|call| {
                !matches!(
                    (call[0].as_str(), call[1].as_str()),
                    ("agent", "list") | ("pane", "get") | ("tab", "list")
                )
            })
            .collect()
    }
    pub fn agents(&self) -> Value {
        read_json(self.root.join("agents.json"))
    }
    pub fn save_agents(&self, agents: &Value) {
        write_json(self.root.join("agents.json"), agents);
    }
    pub fn save_panes(&self, panes: &Value) {
        write_json(self.root.join("panes.json"), panes);
    }
    pub fn agent_args(&self) -> Vec<String> {
        serde_json::from_value(read_json(self.root.join("agent.json"))).unwrap()
    }
    pub fn team(&mut self, kind: Option<&str>, name: Option<&str>, workers: &[u64]) {
        self.env
            .insert("HERDR_PANE_ID".into(), "current-pane".into());
        let cwd = self.root.to_str().unwrap();
        let common = json!({"cwd": cwd, "agent_status": "idle", "tab_id": "boss-tab"});
        let mut boss = common.clone();
        boss["pane_id"] = json!("boss-pane");
        boss["name"] = json!("test-boss");
        boss["agent"] = json!("codex");
        let mut agents = vec![boss];
        for number in workers {
            let mut agent = common.clone();
            agent["pane_id"] = json!(format!("pane-{number}"));
            agent["name"] = json!(format!("test-w{number}"));
            agent["agent"] = json!("claude");
            agents.push(agent);
        }
        let mut current = common;
        current["pane_id"] = json!("current-pane");
        current["agent"] = json!(kind);
        current["tab_id"] = json!("other-tab");
        if let Some(name) = name {
            current["name"] = json!(name);
        }
        if kind.is_some() || name.is_some() {
            agents.push(current.clone());
        }
        self.save_agents(&json!(agents));
        self.save_panes(&json!({"current-pane": current}));
    }
    pub fn lock_path(&self) -> PathBuf {
        let key = format!(
            "{}\0test",
            self.env
                .get("HERDR_SOCKET_PATH")
                .map(String::as_str)
                .unwrap_or("")
        );
        self.root.join(format!(
            "herd-join-{}-{:x}.lock",
            unsafe { libc::getuid() },
            Sha256::digest(key.as_bytes())
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn copy_package(target: &Path) {
    fs::create_dir_all(target.join("bin")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_herdr-shepherd"),
        target.join("bin/herdr-shepherd"),
    )
    .unwrap();
    for name in ["herd-boss", "herd-view"] {
        symlink("herdr-shepherd", target.join("bin").join(name)).unwrap();
    }
    fs::create_dir_all(target.join("skills/herdr-shepherd")).unwrap();
    fs::write(
        target.join("skills/herdr-shepherd/SKILL.md"),
        include_str!("../../skills/herdr-shepherd/SKILL.md"),
    )
    .unwrap();
}

pub fn terminal_supervisor(args: &[String]) -> Result<i32> {
    let settings = || -> Result<libc::termios> {
        let mut state = std::mem::MaybeUninit::uninit();
        if unsafe { libc::tcgetattr(0, state.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { state.assume_init() })
    };
    let before = settings()?;
    let status = Command::new(&args[0]).args(&args[1..]).status()?;
    let after = settings()?;
    let restored = before.c_iflag == after.c_iflag
        && before.c_oflag == after.c_oflag
        && before.c_cflag == after.c_cflag
        && before.c_lflag == after.c_lflag
        && before.c_cc == after.c_cc
        && unsafe {
            libc::cfgetispeed(&before) == libc::cfgetispeed(&after)
                && libc::cfgetospeed(&before) == libc::cfgetospeed(&after)
        };
    println!("\nHERD_TERMIOS={restored}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(if restored {
        status.code().unwrap_or(1)
    } else {
        109
    })
}

pub fn terminal(fixture: &Fixture, args: &[&str], keys: &[u8], size: (u16, u16)) -> (i32, String) {
    let mut master = -1;
    let mut slave = -1;
    let mut window = libc::winsize {
        ws_row: size.0,
        ws_col: size.1,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut window,
            )
        },
        0
    );
    for fd in [master, slave] {
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut command = fixture.command(env::current_exe().unwrap(), &["--pty-supervisor"]);
    command
        .arg(fixture.package.join("bin/herdr-shepherd"))
        .args(args)
        .env("TERM", "xterm-256color")
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave.try_clone().unwrap());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Running(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut bytes = Vec::new();
    let read = |master: &mut File, bytes: &mut Vec<u8>| {
        let mut poll = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 50) } > 0 {
            let mut chunk = [0; 8192];
            match master.read(&mut chunk) {
                Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
                Err(error) => panic!("{error}"),
            }
        }
    };
    while !String::from_utf8_lossy(&bytes).contains("\x1b[?2004h") {
        read(&mut master, &mut bytes);
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert!(Instant::now() < deadline, "The form did not appear.");
    }
    master.write_all(keys).unwrap();
    let mut acknowledged = false;
    let status = loop {
        read(&mut master, &mut bytes);
        let text = String::from_utf8_lossy(&bytes);
        if !acknowledged && text.contains("HERD_TERMIOS=true") {
            acknowledged = true;
            master.write_all(b"\n").unwrap();
        }
        assert!(
            !text.contains("HERD_TERMIOS=false"),
            "The terminal settings were not restored."
        );
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "The terminal setup did not exit. {text}"
        );
    };
    assert!(
        acknowledged,
        "The terminal supervisor did not report its settings."
    );
    (status.code().unwrap(), String::from_utf8(bytes).unwrap())
}
