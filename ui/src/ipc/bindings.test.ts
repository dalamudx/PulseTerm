import { Channel } from "@tauri-apps/api/core";
import { expectTypeOf, it } from "vitest";
import { commands, events, type ProbeConfig } from "./gen/commands";

it("exports exact control and binary channel signatures", () => {
  expectTypeOf(commands.probeMode).returns.toEqualTypeOf<Promise<boolean>>();
  expectTypeOf(commands.probeChannel).parameter(1).toEqualTypeOf<ProbeConfig>();
  expectTypeOf(commands.probeChannel)
    .parameter(2)
    .toEqualTypeOf<Channel<ArrayBuffer>>();
  expectTypeOf(events.probeEcho.listen).toBeFunction();
});

// Never executed. tsc must reject these calls; widening generated arguments to any
// would make @ts-expect-error unused and fail the type check.
function invalidCalls() {
  // @ts-expect-error wrong ID type
  void commands.probeAck(123, 0);
  void commands.probeChannel(
    "id",
    // @ts-expect-error old Rust field spelling is not the wire spelling
    { chunk_bytes: 256, frames: 1, window: 1 },
    new Channel<ArrayBuffer>(),
  );
  void commands.probeChannel(
    "id",
    { chunkBytes: 256, frames: 1, window: 1 },
    // @ts-expect-error JSON number arrays are not binary ArrayBuffers
    new Channel<number[]>(),
  );
}
void invalidCalls;
