import { describe, expect, it } from "vitest";
import { decodeInteractivePrompt } from "./workspace-ssh-interactive";

const packet = {
  version: 1,
  round: "11111111-1111-4111-8111-111111111111",
  name: "保存",
  instructions: "<script>fixture</script>",
  prompts: [{ text: "password", echo: false }],
};
describe("bounded interactive protocol", () => {
  it("preserves literal server copy and echo policy and freezes projection", () => {
    const result = decodeInteractivePrompt(packet);
    expect(result.name).toBe("保存");
    expect(result.instructions).toBe(packet.instructions);
    expect(result.prompts[0]?.echo).toBe(false);
    expect(Object.isFrozen(result.prompts)).toBe(true);
  });
  it("rejects wrong versions, noncanonical rounds, extra properties and wrong echo types", () => {
    for (const input of [
      { ...packet, version: 2 },
      { ...packet, round: packet.round.replace("-4111", "-7111") },
      { ...packet, secret: "fixture" },
      { ...packet, prompts: [{ text: "x", echo: 1 }] },
      { ...packet, prompts: [{ text: "x", echo: false, unexpected: true }] },
    ])
      expect(() => decodeInteractivePrompt(input)).toThrow(
        "ssh.session_protocol",
      );
  });
  it("bounds UTF8 bytes and question count before creating fields", () => {
    for (const input of [
      { ...packet, name: "中".repeat(342) },
      { ...packet, instructions: "中".repeat(1366) },
      { ...packet, prompts: Array(9).fill({ text: "x", echo: false }) },
      { ...packet, prompts: [{ text: "中".repeat(342), echo: false }] },
    ])
      expect(() => decodeInteractivePrompt(input)).toThrow(
        "ssh.session_protocol",
      );
  });
});
