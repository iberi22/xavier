import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { DecisionCard, ADR } from '../src/components/DecisionCard';

const mockADR: ADR = {
  id: 'ADR-001',
  title: 'Test Decision',
  context: 'We need a decision',
  decision: 'Use Vitest',
  consequences_positive: ['Fast', 'Modern'],
  consequences_negative: ['Learning curve'],
  status: 'proposed',
  priority: 'high',
  created_at: '2023-10-27T10:00:00Z',
};

describe('DecisionCard', () => {
  it('renders correctly', () => {
    render(<DecisionCard adr={mockADR} />);

    expect(screen.getByText('ADR-001: Test Decision')).toBeDefined();
    expect(screen.getByText('We need a decision')).toBeDefined();
    expect(screen.getByText('PROPOSED')).toBeDefined();
    expect(screen.getByText('HIGH')).toBeDefined();
  });

  it('calls onAccept when "Accept" is clicked', () => {
    const onAccept = vi.fn();
    render(<DecisionCard adr={mockADR} onAccept={onAccept} />);

    const button = screen.getByText('Accept');
    fireEvent.click(button);

    expect(onAccept).toHaveBeenCalledWith('ADR-001');
  });

  it('renders consequences', () => {
    render(<DecisionCard adr={mockADR} />);

    expect(screen.getByText('Positives')).toBeDefined();
    expect(screen.getByText('Negatives')).toBeDefined();
    expect(screen.getByText('Fast')).toBeDefined();
    expect(screen.getByText('Learning curve')).toBeDefined();
  });
});
