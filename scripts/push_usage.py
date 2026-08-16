#!/usr/bin/env python3
"""把 Claude 用量推给 laoda 小屏（设计文档 §8 / §15 第 6 步）。

工作机上每 5 分钟跑一次：

  */5 * * * * python3 /path/to/scripts/push_usage.py >> /path/to/laoda-push.log 2>&1

环境变量：
  LAODA_PUSH_TOKEN  推送 token，必须与固件编译时一致
  LAODA_PUSH_ADDR   可选，目标地址。默认 `laoda.local`（设备 mDNS 广播，
                    无需知道 DHCP 分配的 IP）；mDNS 不通时可手动指定设备 IP

退出码：0 = 收到设备 ack；1 = 推送失败（设备可能关机/不在同网段，属正常情况）。
"""

import os
import re
import socket
import subprocess
import sys
import time

PORT = 5005
TARGET = os.environ.get("LAODA_PUSH_ADDR", "laoda.local")
CLAUDE_TIMEOUT = 120  # 秒
ACK_TIMEOUT = 5  # 秒


def read_usage() -> tuple[int, int, int]:
    """跑 `claude -p "/usage"` 并解析三个百分比。"""
    out = subprocess.run(
        ["claude", "-p", "/usage"],
        capture_output=True,
        text=True,
        timeout=CLAUDE_TIMEOUT,
        check=True,
    ).stdout

    def pct(pattern: str) -> int:
        m = re.search(pattern, out)
        if not m:
            raise ValueError(f"无法从 /usage 输出解析 {pattern!r}:\n{out}")
        return int(m.group(1))

    return (
        pct(r"Current session: (\d+)%"),
        pct(r"Current week \(all models\): (\d+)%"),
        pct(r"Current week \(Fable\): (\d+)%"),
    )


def main() -> int:
    # 与固件 build.rs 同源：未显式 export 时从项目 .env 读 token/地址
    env_file = os.path.join(os.path.dirname(__file__), "..", ".env")
    if os.path.exists(env_file):
        for line in open(env_file, encoding="utf-8"):
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ.setdefault(k.strip(), v.strip())
    token = os.environ.get("LAODA_PUSH_TOKEN", "")
    if not token:
        print("LAODA_PUSH_TOKEN 未设置", file=sys.stderr)
        return 1
    try:
        session, week, fable = read_usage()
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, ValueError) as e:
        print(f"读取用量失败: {e}", file=sys.stderr)
        return 1
    line = f"laoda1 {token} {session} {week} {fable} {int(time.time())}\n".encode()

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.settimeout(ACK_TIMEOUT)
    try:
        try:
            sock.sendto(line, (TARGET, PORT))
        except socket.gaierror:
            print(
                f"{TARGET} 解析失败：mDNS 未生效（设备未上电/未连网，或工作机无 mDNS 解析）；"
                "可先用 `ping laoda.local` 确认，或 LAODA_PUSH_ADDR=<设备IP> 直推",
                file=sys.stderr,
            )
            return 1
        try:
            data, addr = sock.recvfrom(4)
            if data.strip() == b"ok":
                print(f"推送成功 session={session} week={week} fable={fable} 设备={addr[0]}")
                return 0
            print(f"收到意外回执: {data!r}", file=sys.stderr)
        except socket.timeout:
            print(
                "ack 超时：设备可能关机/不在同网段；若 AP 拦截回程或广播"
                "（车站公共 WiFi 常见），改用单播 LAODA_PUSH_ADDR=<设备IP>，"
                "屏幕数值更新即实际成功（设备 IP 见串口日志 ack 行的 local_address）",
                file=sys.stderr,
            )
        return 1
    finally:
        sock.close()


if __name__ == "__main__":
    sys.exit(main())
