"""Use two real builds to check the first holder after replacing the launch path."""
import fcntl, json, os, shutil, socket, subprocess, sys, tempfile
from pathlib import Path

assert len(sys.argv)==4 and sys.platform in {"linux","darwin"}
out=Path(sys.argv[3]).resolve()
out.mkdir()
with tempfile.TemporaryDirectory(prefix="cf-",dir="/tmp") as tmp:
    root=Path(tmp).resolve()
    env=dict(os.environ,HOME=str(root/"h"),CCCC_HOME=str(root/"c"),CODEX_HOME=str(root/"cx"),CLAUDE_CONFIG_DIR=str(root/"cl"))
    for k in ("HOME","CODEX_HOME","CLAUDE_CONFIG_DIR"): Path(env[k]).mkdir()
    program=root/"cccc"
    project=root/"work";project.mkdir()
    def run(*args,check=True):
        return subprocess.run([program,*args],env=env,text=True,capture_output=True,timeout=60,check=check)
    def rpc(path,body):
        with socket.socket(socket.AF_UNIX) as conn:
            conn.settimeout(6)
            conn.connect(str(path))
            conn.sendall(json.dumps(body).encode()+b"\n")
            return json.loads(conn.makefile("rb").readline())
    home=Path(env["CCCC_HOME"])
    socket_path=home/"daemon/ccccd.sock"
    holders=home/"daemon/session-owners"
    def live_sources():
        result=[]
        for lock in sorted(holders.glob("*/owner.lock")):
            with lock.open("rb") as f:
                try: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
                except BlockingIOError: pass
                else: continue
            address=json.loads((lock.parent/"address.json").read_text())
            reply=rpc(address["socket"],{"op":"hello","contract":address["contract"]})
            assert reply["instance"]==address["instance"] and reply["pid"]==address["pid"]
            result.append(reply["build"]["source_id"])
        return result
    def daemon_source():
        return rpc(socket_path,{"v":1,"op":"ping","args":{}})["result"]["build"]["source_id"]
    shutil.copy2(sys.argv[1],program)
    before=json.loads(run("update","--self-check").stdout)["build"]["source_id"]
    try:
        run("daemon","start")
        gid=json.loads(run("attach",str(project)).stdout)["group_id"]
        run("actor","add","m1","--group",gid,"--runtime","custom","--command","/bin/cat")
        assert not live_sources()
        shutil.copy2(sys.argv[2],root/"replacement")
        (root/"replacement").replace(program)
        after=json.loads(run("update","--self-check").stdout)["build"]["source_id"]
        assert before and after and before!=after and daemon_source()==before
        attempt=run("actor","start","m1","--group",gid,check=False)
        sources=live_sources()
        observed={"platform":sys.platform,"daemon":before,"installed":after,"start_rc":attempt.returncode,
                  "owners_after_start":sources, "error":(attempt.stderr+attempt.stdout).strip() if attempt.returncode else ""}
        (out/"before-restart.json").write_text(json.dumps(observed,indent=2)+"\n")
        if attempt.returncode==0:
            assert sources==[before], observed
        else:
            assert sys.platform=="darwin" and "restart CCCC" in observed["error"] and not sources, observed
        run("daemon","stop")
        run("daemon","start")
        assert daemon_source()==after
        run("actor","start","m1","--group",gid)
        assert live_sources()==[after]
        stopped=json.loads(run("daemon","stop").stdout)["receipt"]
        assert stopped["mode"]=="stop" and not stopped["unconfirmed"] and not live_sources()
        observed.update(restarted_daemon=after,restarted_holder=after,ordinary_stop_confirmed=True)
        (out/"result.json").write_text(json.dumps(observed,indent=2)+"\n")
        print(sys.platform, "first holder and restart verified")
    finally: run("daemon","stop",check=False)
