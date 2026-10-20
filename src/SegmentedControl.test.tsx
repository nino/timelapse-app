import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import React, { useState } from 'react';

import { SegmentedControl } from './SegmentedControl';

function Picker(): React.ReactNode {
  const [kind, setKind] = useState<'bug' | 'feature'>('bug');
  return (
    <SegmentedControl
      label="Kind"
      options={[
        { value: 'bug', label: 'Bug' },
        { value: 'feature', label: 'Feature request' },
      ]}
      value={kind}
      onChange={setKind}
    />
  );
}

describe('SegmentedControl', () => {
  it('places the pill without animating, then marks the choice picked', () => {
    const { container } = render(<Picker />);
    const pill = container.querySelector<HTMLElement>('[aria-hidden]');

    // Placed on the first render, so later moves animate.
    expect(pill?.dataset.placed).toBe('');
    expect(pill?.style.opacity).toBe('1');
    expect(screen.getByRole('button', { name: 'Bug' })).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Feature request' }));

    expect(screen.getByRole('button', { name: 'Feature request' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Bug' })).toHaveAttribute('aria-pressed', 'false');
  });
});
