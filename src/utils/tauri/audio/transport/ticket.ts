import type { EventMeta } from "../protocol";
import type { CommandTicket } from "./types";

/** A ticket whose command number is assigned when the command is actually sent. */
export class PendingTicket implements CommandTicket {
  /** 0 until sent. */
  cmd = 0;
  /** The backend will never see it (the send failed or the transport shut down). */
  dropped = false;

  acknowledgedBy(meta: EventMeta): boolean {
    return this.dropped || (this.cmd !== 0 && meta.ack >= this.cmd);
  }
}
