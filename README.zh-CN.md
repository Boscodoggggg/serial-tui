<h1 align="center">
  <img src="docs/assets/serial-tui-banner.webp" alt="Serial TUI 串口调测台" width="960">
</h1>

<p align="center">
  <strong>Linux 终端里的纯键盘串口调测台</strong><br>
  选串口、发文本或 HEX、看设备回包，一屏完成。
</p>

<p align="center">
  <kbd>🐧 Linux · x86_64 / ARM64 / ARMv7</kbd> &nbsp; <kbd>⌨️ 纯键盘</kbd> &nbsp; <kbd>🌐 中文 / English</kbd> &nbsp; <a href="LICENSE"><kbd>MIT</kbd></a>
</p>

<p align="center">
  <a href="#快速开始">⬇️ 快速开始</a> · <a href="#连接设备">🔌 连接设备</a> · <a href="#快捷键">⌨️ 快捷键</a> · <a href="README.md">English</a>
</p>

## 终端实录

![中文串口调测台实录，展示波特率选择和收发、仅接收、仅发送三种模式。](docs/assets/terminal-zh.gif)

画面录自 Linux 发布版，演示连接设备前的串口设置和模式切换。

## 快速开始

在要运行程序的 Linux 机器上执行 `uname -m`，按输出选择可执行文件。

| `uname -m` | 下载 |
| --- | --- |
| `x86_64` | [Linux x86_64](https://github.com/Boscodoggggg/serial-tui/releases/latest/download/serial-tui-linux-x86_64) |
| `aarch64` | [Linux ARM64](https://github.com/Boscodoggggg/serial-tui/releases/latest/download/serial-tui-linux-aarch64) |
| `armv7l` 或 `armv8l` | [Linux ARMv7（32 位，硬浮点）](https://github.com/Boscodoggggg/serial-tui/releases/latest/download/serial-tui-linux-armv7) |

ARM64 板卡可以直接运行下面的命令。其他架构请换成表中对应的下载地址。

```sh
curl -fL https://github.com/Boscodoggggg/serial-tui/releases/latest/download/serial-tui-linux-aarch64 -o serial-tui
chmod +x ./serial-tui
./serial-tui --lang zh
```

按 `F6` 切换中英文，按 `Ctrl+Q` 退出。

## 连接设备

```sh
./serial-tui --port /dev/ttyUSB0 --baud 115200 --lang zh
```

1. 插上串口适配器，把上面的设备路径和波特率换成实际值。按 `F2` 进入设置，用方向键和 `Enter` 可以调整串口、波特率、数据位、停止位、校验位和流控。
2. 确认串口后，设置区会高亮「连接」或「应用 / 重连」。再按一次 `Enter` 打开串口，也可以先用 `↑` 返回上方修改参数。设备未出现在列表里时，可选择「输入设备路径」。
3. 按 `F3` 选择收发模式。需要发送时，用 `Tab` 切到发送框，输入内容后按 `Enter`。

首次连接成功时，收发和仅接收模式会聚焦日志区，仅发送模式会聚焦发送框。重连时，或在连接过程中手动移动焦点后，会保留当前位置。

在插着串口设备的 Linux 机器上运行程序。遇到权限错误时，可用 `ls -l /dev/ttyUSB0` 查看设备节点，并使用有访问权限的账号运行。建议使用至少 80 列 × 24 行的终端。

「刷新串口」根据 Linux 设备信息列出 USB 转串口和板载 UART。同一设备只显示一项，有 `by-id` 或 `by-path` 链接时优先使用这些稳定路径。在串口弹窗里，按 `PgUp` / `PgDn` 翻页，按 `F5` 原地刷新。选中的设备仍在时会保留高亮。选择「连接」后才会尝试打开设备。

工具会把上次成功连接的参数保存到 `$XDG_STATE_HOME/serial-tui/last-connection.json`，默认位置是 `~/.local/state/serial-tui/last-connection.json`。`--port` 和 `--baud` 可以覆盖本次启动的对应参数，仍需手动选择「连接」。应用新参数失败时，工具会保留原连接，或尝试按旧参数恢复。

| 模式 | 行为 |
| --- | --- |
| **收发** | 在同一份日志里看发送内容和设备回包。 |
| **仅接收** | 查看设备输出，发送区收起，循环发送停止。 |
| **仅发送** | 单次或循环发送指令，收到的字节会被丢弃。 |

## 快捷键

| 按键 | 操作 |
| --- | --- |
| `F2` / `F3` | 进入串口设置 / 切换收发模式 |
| `F4` / `F6` | 暂停或恢复跟随 / 切换中英文 |
| `F8` | 导出当前保留的日志到新文件，收发内容以 HEX 保存 |
| `F9` | 开始或停止把新收到和发出的数据连续写入文件 |
| `Tab` / `Shift+Tab` | 在串口设置、日志、发送选项和发送框之间切换 |
| 日志区的 `PgUp` / `PgDn` · `←` / `→` · `End` | 翻页、横向查看长行、回到最新日志 |
| 日志区的 `H` | 切换文本、HEX、HEX + ASCII 显示 |
| 日志区的 `/`、`n` / `N` | 搜索已保留的日志，跳到下一处或上一处匹配 |
| 发送选项区的 `←` / `→` 和 `Enter` | 设置文本或 HEX、行尾、常用指令和循环间隔 |
| `F7` / `F10` | 打开常用指令 / 选择循环间隔或停止循环发送 |
| 发送框的 `↑` / `↓` · `Enter` · `Ctrl+U` | 调用历史、发送、清空输入 |
| `Esc` / `Ctrl+Q` | 关闭当前弹窗 / 退出程序 |

暂停后仍可回看当时的日志，串口会继续接收。实时日志保留最近 2,000 条记录。搜索弹窗里按 `Tab` 切换文本和 HEX，按 `F3` 切换到仅接收。搜索逐条匹配当前缓冲区或暂停时保留的记录。在日志区按 `Esc` 清除搜索，按 `End` 回到实时跟随。

`F7` 打开内置指令，选中后填入发送框。循环发送时，先连接串口并填好内容，再按 `F10` 选择间隔。再按一次 `F10`，或在循环选项上按 `Enter`，即可停止并撤销仍在排队的循环帧。

长时间调测时，按 `F9` 输入新文件名并确认。即使屏幕暂停跟随或旧日志被滚动淘汰，文件也会继续记录时间戳、方向和 HEX 字节。录制遵循当前收发模式，需要双向记录时请选择「收发」。再按一次 `F9` 即可停止并保存。

## 许可证

[MIT](LICENSE)
