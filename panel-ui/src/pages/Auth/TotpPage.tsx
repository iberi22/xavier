import React, { useState } from 'react';
import { useAuthStore } from '../../store/authStore';

export const TotpPage: React.FC = () => {
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const mfaEmail = useAuthStore(state => state.mfaEmail);
  const setAuth = useAuthStore(state => state.setAuth);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    try {
      const response = await fetch('/v1/auth/totp/verify', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: mfaEmail, code }),
      });

      const data = await response.json();

      if (response.ok) {
        setAuth(data.user, data.access_token, data.refresh_token);
      } else {
        setError(data.error || 'Verification failed');
      }
    } catch (err) {
      setError('Connection error');
    }
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-dvh bg-black text-[#39ff14] font-mono">
      <form onSubmit={handleSubmit} className="w-full max-w-md p-8 border border-[#39ff14]/30 rounded-lg bg-black/50">
        <h2 className="text-2xl mb-2 text-center tracking-tighter uppercase">2FA VERIFICATION</h2>
        <p className="text-xs text-center opacity-70 mb-6 uppercase">Enter the 6-digit code from your authenticator app</p>
        {error && <div className="mb-4 text-red-500 text-sm border border-red-500/30 p-2">{error}</div>}
        <div className="mb-6">
          <input
            type="text"
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="000000"
            maxLength={6}
            className="w-full bg-black border border-[#39ff14]/30 p-3 text-2xl text-center tracking-[1em] focus:outline-none focus:border-[#39ff14]"
            required
          />
        </div>
        <button
          type="submit"
          className="w-full bg-[#39ff14] text-black font-bold py-2 uppercase tracking-widest hover:bg-[#39ff14]/80 transition-colors"
        >
          Verify Identity
        </button>
      </form>
    </div>
  );
};
