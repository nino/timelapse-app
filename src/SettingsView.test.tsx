import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

import { SettingsView } from './SettingsView';
import { mocked } from './test/mocked';

// invoke is replaced with a mock in src/test/setup.ts.

/** Answers each command with `reply`. */
function answer(reply: (cmd: string, args?: unknown) => Promise<unknown>): void {
  mocked(invoke).mockImplementation(reply as typeof invoke);
}

beforeEach(() => {
  mock.clearAllMocks();
  mocked(invoke).mockReset();
});

describe('SettingsView', () => {
  it('shows the saved setting', async () => {
    mocked(invoke).mockResolvedValue({ updateAutomatically: false });
    render(<SettingsView />);

    const box = await screen.findByRole('checkbox', { name: /Update automatically/ });
    expect(box).not.toBeChecked();
    expect(invoke).toHaveBeenCalledWith('get_settings');
  });

  it('saves a change as soon as it is made', async () => {
    answer(async (cmd, args) =>
      cmd === 'get_settings'
        ? { updateAutomatically: true }
        : { updateAutomatically: (args as { enabled: boolean }).enabled },
    );
    render(<SettingsView />);

    const box = await screen.findByRole('checkbox', { name: /Update automatically/ });
    expect(box).toBeChecked();
    fireEvent.click(box);

    expect(box).not.toBeChecked();
    expect(invoke).toHaveBeenCalledWith('set_update_automatically', { enabled: false });
  });

  it('puts the box back and says so when saving fails', async () => {
    answer(async (cmd) => {
      if (cmd === 'get_settings') return { updateAutomatically: true };
      throw 'disk full';
    });
    render(<SettingsView />);

    const box = await screen.findByRole('checkbox', { name: /Update automatically/ });
    fireEvent.click(box);

    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('disk full'));
    expect(box).toBeChecked();
  });
});
