import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ScheduledTask } from "../../types";
import { ScheduleCell } from "./Jobs";

const backup = {
  task: "backup",
  label: "Back up the database",
  schedule: "weekly",
  uses_ai: false,
  chosen: false,
  options: null,
  keep: 4,
  limit: null,
  last_run: null,
  next_run_at: null,
  next_after: null,
} as ScheduledTask;

describe("Settings, Jobs: backups kept (security review L5)", () => {
  it("is chosen on the computer UpNext runs on", () => {
    render(
      <ScheduleCell
        row={backup}
        rows={[backup]}
        labels={{}}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("Backups kept")).toBeTruthy();
  });

  it("is only shown on another device, which the server refuses it", () => {
    render(
      <ScheduleCell
        row={backup}
        rows={[backup]}
        labels={{}}
        here={false}
        onChange={vi.fn()}
      />,
    );
    expect(screen.queryByLabelText("Backups kept")).toBeNull();
    expect(
      screen.getByTitle("Set on the computer UpNext runs on").textContent,
    ).toBe("4");
  });
});
