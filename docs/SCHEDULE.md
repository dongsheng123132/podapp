# 定时跑一条流程

**泊舟不自带定时器。** 机器上已经有调度器了：Windows 任务计划、cron、
GitHub Actions、以及 AI agent 自己的定时任务。自带一个意味着浮舱必须常驻
（关了就不跑）、要自己处理错过的时间点、要自己存任务表、重启后要恢复 ——
全是别人已经做好而且做得更好的事。

泊舟提供的是**能被任何调度器调用的一个面**：`podapp-run`。

## 三条命令就够

```bash
podapp-run install <目录或.pod>          # 无头机器上没有浮舱可以拖
podapp-run check   morning.flow.json     # 只验不跑
podapp-run flow    morning.flow.json --json
```

## 退出码（调度器靠它分支）

| 码 | 含义 | 调度器该怎么办 |
|---|---|---|
| `0` | 跑完了 | 收工 |
| `1` | 用法或输入不对（流程验不过、JSON 坏了） | **别重试**，是你给错了 |
| `2` | 某一步执行失败 | 可以重试 |
| `3` | **停在等确认** | **别重试** —— 重试只会每天重跑前面几步。去叫人 |

`3` 是刻意分出来的：命令行**不代替人点头**。一条流程里有声明了
`confirmation` 的动作，无人值守时就该停下并说清楚，而不是替用户按下确认。
真要在定时里跳过它，用 `--from N` 明写 —— 那样"跳过了确认"是记录在案的。

## 输出约定

- **stdout 只有结果**，日志和人话一律 stderr。`podapp-run ... --json | jq` 永远能解析
- `--json` 给 `{ok, data}` 稳定形状，单行
- 不带颜色、不带 spinner —— 非 TTY 下那些是垃圾字符

## Windows 任务计划

```powershell
schtasks /create /tn "podapp-morning" /sc daily /st 08:00 /tr `
  "\"C:\Program Files\泊舟\podapp-run.exe\" flow C:\flows\morning.flow.json --json"
```

## 跟 AI agent 的定时任务搭

这是更常见的用法：**agent 的定时任务调泊舟**，不是反过来。

```
每天 08:00（agent 的定时任务）
  → agent 跑 podapp-run flow pull-site-data.flow.json --json
  → 拿到确定性的数据产物（路径在返回里）
  → agent 自己读产物、写报告
```

分工是清楚的：**泊舟负责确定性那半（拉取、转换、校验、落盘），
AI 负责生成和理解。** 泊舟一个模型都不调，密钥和账单都不经过它。

产物会进收件箱，所以 agent 也可以走 MCP 的 `podapp_inbox_recent`
主动去取「刚才那一轮产出了什么」，而不用自己记路径。

## 想让 AI 定时干活，但不想写流程？

那就让 agent 的定时任务直接调它自己 —— 那不需要泊舟。泊舟值得掺进来的
只有一种情况：**这件事里有一段必须确定性、必须可复现、必须能被验的**。
比如「抠图」「校验图集」「打包」「导出」。没有这样一段，就别加一层。
