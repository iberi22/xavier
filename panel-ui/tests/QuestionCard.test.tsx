import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import QuestionCard, { Question } from '../src/components/QuestionCard';

const mockQuestion: Question = {
  id: '1',
  title: 'Test Question',
  content: 'This is a test question',
  priority: 'urgent',
  project: 'PROJECT-A',
  category: 'tecnico',
  status: 'open',
  created_at: '2023-10-27T10:00:00Z',
  updated_at: '2023-10-27T10:00:00Z',
};

describe('QuestionCard', () => {
  it('renders correctly', () => {
    render(<QuestionCard question={mockQuestion} />);

    expect(screen.getByText('Test Question')).toBeDefined();
    expect(screen.getByText('This is a test question')).toBeDefined();
    expect(screen.getByText('URGENTE')).toBeDefined();
    expect(screen.getByText('TECNICO')).toBeDefined();
  });

  it('calls onStatusChange when "Responder" is clicked', () => {
    const onStatusChange = vi.fn();
    render(<QuestionCard question={mockQuestion} onStatusChange={onStatusChange} />);

    const button = screen.getByText('Responder');
    fireEvent.click(button);

    expect(onStatusChange).toHaveBeenCalledWith('1', 'answered');
  });

  it('renders answer when provided', () => {
    const answeredQuestion = { ...mockQuestion, answer: 'The answer', status: 'answered' as const };
    render(<QuestionCard question={answeredQuestion} />);

    expect(screen.getByText('RESPUESTA')).toBeDefined();
    expect(screen.getByText('The answer')).toBeDefined();
  });
});
