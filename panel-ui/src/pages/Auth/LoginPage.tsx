import React, { useState } from 'react';
import { useAuthStore } from '../../store/authStore';

export const LoginPage: React.FC = () => {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const setAuth = useAuthStore(state => state.setAuth);
  const setMfaRequired = useAuthStore(state => state.setMfaRequired);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    try {
      const response = await fetch('/v1/auth/login', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
      });

      const data = await response.json();

      if (response.status === 202 && data.status === 'mfa_required') {
        setMfaRequired(email);
      } else if (response.ok) {
        setAuth(data.user, data.access_token, data.refresh_token);
      } else {
        setError(data.error || 'Login failed');
      }
    } catch (err) {
      setError('Connection error');
    }
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-screen bg-black text-[#39ff14] font-mono">
      <form onSubmit={handleSubmit} className="w-full max-w-md p-8 border border-[#39ff14]/30 rounded-lg bg-black/50">
        <h2 className="text-2xl mb-6 text-center tracking-tighter uppercase">Xavier Login</h2>
        {error && <div className="mb-4 text-red-500 text-sm border border-red-500/30 p-2">{error}</div>}
        <div className="mb-4">
          <label className="block text-xs uppercase mb-1">Email</label>
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="w-full bg-black border border-[#39ff14]/30 p-2 text-sm focus:outline-none focus:border-[#39ff14]"
            required
          />
        </div>
        <div className="mb-6">
          <label className="block text-xs uppercase mb-1">Password</label>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full bg-black border border-[#39ff14]/30 p-2 text-sm focus:outline-none focus:border-[#39ff14]"
            required
          />
        </div>
        <button
          type="submit"
          className="w-full bg-[#39ff14] text-black font-bold py-2 uppercase tracking-widest hover:bg-[#39ff14]/80 transition-colors"
        >
          Access Terminal
        </button>
      </form>
    </div>
  );
};
