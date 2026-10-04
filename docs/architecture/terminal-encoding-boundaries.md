# 字符编码与终端恢复边界：已采纳双坐标协议

状态：Accepted（用户确认方案 A：版本化双坐标）。v2、actor和注册层已实现，并取得固定UTF8/GBK真实WebView证据；不等于生产字符编码或远端设备验收。v1/parsed_bytes仍保留原始source语义。

## 已知约束与证据

- ADR-027：原始字节先做二进制传输（如 Zmodem）分类，文本在后端转成 UTF-8，parser/前端只接 UTF-8；输入反向转换。
- `SessionState.parsed_bytes`按原始source递增，utf8_bytes按归一化输出递增；验证特性有strictGBK decoder，跨lease存活。生产binary preclassifier/charset切换尚未接入。
- v1 parsed_bytes保留原始输出边界并拒绝非UTF8会话；attach_v2分别携带source/UTF8游标，不能用一个替代另一个。
- 原单坐标假设已迁移：driver/frontend Data连续性按utf8_bytes/bodylen，source单调推进并支持无显示Marker；ACK只按实际wire信用释放。
- 实测固定 GBK Chinese/ANSI/DSR fixture 原始 33 bytes、UTF-8 43 bytes；全部 1..33 分块保持 scalar、cells 和 query replies。直接接一个 decoder 而保留单坐标会使下一块 offset/EOF/summary 不一致。
- 半个 GBK 字符可先消耗原始 1 byte、暂不产生 UTF-8；下一 byte 可能产生 3 UTF-8 bytes。快照只需要当前显示状态，decoder pending 状态仍由同一个 backend session 保留。

## A：双坐标都进入版本化协议（推荐）

保留原始 source counter，另有 normalized UTF-8 display cursor。actor、恢复 boundary 和 live/marker/end/repair 元数据明确携带两个量。Data连续性按 UTF-8 body 长度，source counter在完成对应源块时推进；分片中间不能假称每个UTF-8 byte有一对一raw byte地址。无显示输出但decoder消费源数据时用同队列纯state marker发布，禁止用强制flush改变终端。

- 原始日志/二进制分类/排错与恢复来源可以保留现有raw语义。
- render只理解UTF-8，ACK额度仍按实际wire frame字节，不用raw或normalized累计计数代替credit。
- generation/revision/两坐标共同比较，旧或仅一个坐标一致的摘要不能清风险；encoding切换需要明确decoder pending处理和新边界。
- 需要attachment v2或显式可协商扩展：Rust sender/driver、TS receiver/consumer、headless/DOM/CLI fixtures和public DTO同步迁移；旧v1必须明确拒绝非UTF-8而非误解析。不混用两版frame。
- 已获用户确认；v2编解码/actor/driver/registry已实现。固定UTF8-v2和显式encoding-probe GBK窗口验证通过，默认桌面无validation feature，不开放生产GBK；历史持久化仍未承诺迁移。

示意（不是最终binary layout）：

```text
Begin: raw_source_at=33, utf8_at=43, revision=R, generation=G
Data:  utf8_offset=43, body=<UTF8>, raw_source_after=<monotonic>
Marker/End: raw_source_at, utf8_at, revision
ACK: generation + sequence（actual frame credit unchanged）
```

## B：协议只按显示UTF-8计数，raw仅后台另记

重新定义现有`parsed_bytes`为parser实际UTF-8字节数；raw counter单独保留在backend诊断/原始日志，不进入前端恢复/live协议。

- v1 bodylen/offset/credit逻辑基本可复用，UI不处理第二坐标。
- 原始日志仍可保留，但source与render恢复边界不能仅凭wire对应；codec pending/source-byte追踪在后台管理。
- 会改变已记录parsed_bytes的公开语义，需更名/文档与DTO说明，不能只修改注释假称兼容。未来需要source定位/原始传输恢复时还需扩协议。
- 更少初始wire工作，但后续关联原始日志/编码临时切换/双坐标诊断的协议债务更多。

## 两者共有的实现要求

1. 无用户内容的负向用例：source/display坐标不符、越界/回退、旧generation ACK、字节跨度不能用number、unknown version拒绝。
2. 输入与terminal query回复都反向编码；不可表示的输入明确失败，不发HTML实体替代字符。输出非法/EOF半字符处理单独记录策略，不能误报干净EOF。
3. decoder与binary preclassifier归backend生命周期，detach不重置；history/restore不是原始源bytes，不能混计两坐标。
4. 背压同时限制raw、归一化展开、actor队列、restore与driver in-flight，不能只限制wire后无限积累decoded text。
5. 测试GBK lead-byte跨capture/rebind、无显示输出的source推进、UTF8/GBK切换、EOF排空、summary/repair一致性与真实SSH/PTY渲染链。不能把S4纯codec通过当生产会话编码通过。
