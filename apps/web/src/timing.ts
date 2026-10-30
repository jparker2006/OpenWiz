// PROVISIONAL engineering choices, made on fixtures. They are not targets:
// no performance baseline on a real account has been measured.

export type Timing = {
  /** From the end of one list read to the start of the next. */
  listIntervalMs: number;
  /** From the end of one detail read to the start of the next, while open. */
  detailIntervalMs: number;
  /** After this long, the page says a read is late (disconnected). The read itself goes on. */
  requestTimeoutMs: number;
  /**
   * How long a read keeps its lane from when it was sent, unless the server
   * answered it: the bound on openwiz-server's work for one request, plus a
   * margin for the network. The read is given up at this point.
   */
  laneHoldMs: number;
  /** Delays after 1, 2, 3, ... failures in a row. The last is the cap. */
  backoffMs: readonly number[];
  /** The last good snapshot is stale this long after its response arrived. */
  staleAfterMs: number;
};

export const TIMING: Timing = {
  listIntervalMs: 10_000,
  detailIntervalMs: 15_000,
  requestTimeoutMs: 45_000,
  // contracts/web-api.v1.md: 10 s for the request head, then a 40 s deadline
  // (above the core's 30 s run budget plus the 5 s slot wait), then 10 s to
  // write the response: 60 s from the request's arrival. Plus 10 s for transit.
  // The contract does not bound transit, or a run that ignores the core's
  // budget ("A run past it keeps its slot"): a known gap.
  laneHoldMs: 70_000,
  backoffMs: [2_000, 4_000, 8_000, 16_000, 30_000],
  staleAfterMs: 30_000,
};
