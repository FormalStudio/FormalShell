{ rustCommon, testers }:

# fs-audio's PipeWire client against a real pipewire + wireplumber in a VM,
# read back through wpctl and pactl. The crate's clippy and unit tests run
# in the probe's build, the only place its pipewire feature compiles.
let
  probe = rustCommon.craneLib.mkCargoDerivation (rustCommon.checkArgs // {
    pname = "fs-audio-probe";
    buildPhaseCargoCommand = ''
      cargo clippy --release -p fs-audio --features pipewire --all-targets -- -D warnings
      cargo test --release -p fs-audio --features pipewire
      cargo build --release -p fs-audio --features pipewire --example fs-audio-probe
    '';
    installPhaseCommand = ''
      install -Dm755 target/release/examples/fs-audio-probe $out/bin/fs-audio-probe
    '';
  });
in
testers.runNixOSTest {
  name = "fs-audio";

  nodes.machine = { pkgs, ... }: {
    services.pipewire = {
      enable = true;
      pulse.enable = true;
      wireplumber.enable = true;
    };
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      linger = true;
    };
    environment.systemPackages = [ probe pkgs.pulseaudio pkgs.wireplumber pkgs.pipewire pkgs.sox pkgs.jq ];
  };

  testScript = ''
    import json

    def user(cmd):
        return machine.succeed(f"su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; {cmd}'")

    def dump():
        return json.loads(user("fs-audio-probe dump"))

    def node(d, name):
        return next(n for n in d["nodes"] if n["name"] == name)

    def wpctl_volume(node_id):
        out = user(f"wpctl get-volume {node_id}").split()
        return float(out[1]), "[MUTED]" in out

    machine.wait_for_unit("user@1000.service")
    machine.systemctl("start pipewire.socket pipewire-pulse.socket wireplumber.service", "alice")
    machine.wait_for_unit("wireplumber.service", "alice")
    machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 pactl info'")

    with subtest("enumerate"):
        user("pactl load-module module-null-sink sink_name=fs_a sink_properties=device.description=SinkA")
        user("pactl load-module module-null-sink sink_name=fs_b sink_properties=device.description=SinkB")
        machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 wpctl status' | grep -q SinkB")
        d = dump()
        a, b = node(d, "fs_a"), node(d, "fs_b")
        for n, desc in ((a, "SinkA"), (b, "SinkB")):
            assert n["type"] == "AudioSink", n
            assert n["description"] == desc, n
            assert n["is_sink"] and not n["is_stream"] and n["ready"], n
            assert len(n["volumes"]) == len(n["channels"]) == 2, n
            assert abs(n["volume"] - wpctl_volume(n["id"])[0]) < 0.005, n

    with subtest("volume set and read back"):
        user("fs-audio-probe set-volume fs_a 0.5")
        vol, muted = wpctl_volume(a["id"])
        assert abs(vol - 0.5) < 0.005 and not muted, (vol, muted)
        assert abs(node(dump(), "fs_a")["volume"] - 0.5) < 0.005
        user(f"wpctl set-volume {a['id']} 0.3")
        n = node(dump(), "fs_a")
        assert abs(n["volume"] - 0.3) < 0.005, n
        assert all(abs(v - 0.3) < 0.005 for v in n["volumes"]), n

    with subtest("mute"):
        user("fs-audio-probe mute fs_a true")
        assert wpctl_volume(a["id"])[1]
        assert node(dump(), "fs_a")["muted"]
        user("fs-audio-probe mute fs_a false")
        assert not wpctl_volume(a["id"])[1]
        user(f"wpctl set-mute {a['id']} 1")
        assert node(dump(), "fs_a")["muted"]

    with subtest("default sink"):
        user("fs-audio-probe set-default-sink fs_b")
        assert user("pactl get-default-sink").strip() == "fs_b"
        d = dump()
        assert d["defaults"]["sink"] == "fs_b" and d["defaults"]["configured_sink"] == "fs_b", d["defaults"]
        assert d["default_sink"] == b["id"], d
        user("pactl set-default-sink fs_a")
        d = dump()
        assert d["defaults"]["sink"] == "fs_a" and d["default_sink"] == a["id"], d["defaults"]
        user("fs-audio-probe set-default-sink fs_b")
        assert user("pactl get-default-sink").strip() == "fs_b"

    with subtest("stream appears linked to the default sink"):
        user("sox -n -r 48000 -c 2 /tmp/tone.wav synth 30 sine 440")
        user("pw-play /tmp/tone.wav >/dev/null 2>&1 &")
        status, out = machine.execute("su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; fs-audio-probe wait-stream pw-play 15'")
        if status != 0:
            print(out)
            for o in json.loads(user("pw-dump")):
                info = o.get("info") or {}
                if (info.get("props") or {}).get("media.class") == "Stream/Output/Audio":
                    print(o["id"], json.dumps(info.get("params", {}).get("Props")))
        assert status == 0
        s = json.loads(out)
        assert s["type"] == "AudioOutStream" and s["is_stream"], s
        assert s["target"] == "fs_b", s
        assert len(s["volumes"]) > 0, s

    with subtest("a blip plays on a stream of its own, gone once it lingered"):
        # One su session for the lot: a background child of a `su -c` that
        # has returned is hung up on.
        machine.succeed("""cat > /tmp/sfx.sh <<'EOF'
    pkill pw-play
    parec -d fs_b.monitor --raw --format=s16le --rate=48000 --channels=1 > /tmp/sfx.raw &
    rec=$!
    sleep 1
    fs-audio-probe play activate 1.0 15 > /tmp/sfx-dump.json &
    probe=$!
    up=0
    gone=0
    for i in $(seq 1 100); do
      if pw-cli ls Node | grep -q formalshell-sfx; then up=1; break; fi
      sleep 0.1
    done
    for i in $(seq 1 100); do
      if ! pw-cli ls Node | grep -q formalshell-sfx; then gone=1; break; fi
      sleep 0.1
    done
    kill -0 $probe && alive=1 || alive=0
    wait $probe
    kill $rec
    echo "$up $gone $alive" > /tmp/sfx-seen
    EOF""")
        user("bash /tmp/sfx.sh")
        up, gone, alive = user("cat /tmp/sfx-seen").split()
        assert up == "1", "no formalshell-sfx node while the blip played"
        assert gone == "1", "the stream outlived its linger"
        assert alive == "1", "the probe was gone before the stream was"
        d = json.loads(user("cat /tmp/sfx-dump.json"))
        assert not any(n["name"] == "formalshell-sfx" for n in d["nodes"]), d
        stat = user("sox -t raw -r 48000 -e signed -b 16 -c 1 /tmp/sfx.raw -n stat 2>&1")
        peak = next(float(l.split(":")[1]) for l in stat.splitlines() if l.startswith("Maximum amplitude"))
        print(f"blip capture peak {peak:.3f}")
        assert peak > 0.3, stat
  '';
}
