import React from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface OpenUIRendererProps {
  content: string;
}

export function OpenUIRenderer({ content }: OpenUIRendererProps) {
  return (
    <div className="openui-renderer text-sm font-sans leading-relaxed text-white/80">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          p: ({ node, ...props }) => (
            <p className="mb-4 last:mb-0" {...props} />
          ),
          a: ({ node, ...props }) => (
            <a className="text-[#39ff14] hover:underline" {...props} />
          ),
          ul: ({ node, ...props }) => (
            <ul className="list-disc pl-5 mb-4 last:mb-0 space-y-1" {...props} />
          ),
          ol: ({ node, ...props }) => (
            <ol className="list-decimal pl-5 mb-4 last:mb-0 space-y-1" {...props} />
          ),
          li: ({ node, ...props }) => (
            <li className="pl-1" {...props} />
          ),
          h1: ({ node, ...props }) => (
            <h1 className="text-xl font-bold text-white mb-4 mt-6 border-b border-white/10 pb-2" {...props} />
          ),
          h2: ({ node, ...props }) => (
            <h2 className="text-lg font-semibold text-white mb-3 mt-5" {...props} />
          ),
          h3: ({ node, ...props }) => (
            <h3 className="text-base font-medium text-white mb-2 mt-4" {...props} />
          ),
          blockquote: ({ node, ...props }) => (
            <blockquote className="border-l-2 border-[#39ff14]/50 pl-4 italic text-white/60 mb-4 bg-[#39ff14]/[0.02] py-2 rounded-r" {...props} />
          ),
          code({ node, inline, className, children, ...props }: any) {
            const match = /language-(\w+)/.exec(className || '');
            return !inline ? (
              <div className="my-4 rounded-md overflow-hidden bg-black/40 border border-white/10 shadow-lg backdrop-blur-md">
                <div className="flex items-center px-4 py-2 bg-white/[0.03] border-b border-white/5 text-[10px] text-white/40 uppercase tracking-wider">
                  {match ? match[1] : 'Code'}
                </div>
                <pre className="p-4 overflow-x-auto text-[13px] font-mono leading-normal text-white/70">
                  <code className={className} {...props}>
                    {children}
                  </code>
                </pre>
              </div>
            ) : (
              <code className="bg-black/30 text-[#39ff14] px-1.5 py-0.5 rounded font-mono text-[0.85em] border border-white/5" {...props}>
                {children}
              </code>
            );
          },
          table: ({ node, ...props }) => (
            <div className="overflow-x-auto my-4 rounded-md border border-white/10 bg-black/20 backdrop-blur-sm">
              <table className="min-w-full text-left text-sm" {...props} />
            </div>
          ),
          thead: ({ node, ...props }) => (
            <thead className="border-b border-white/10 bg-white/[0.02]" {...props} />
          ),
          th: ({ node, ...props }) => (
            <th className="px-4 py-3 font-medium text-white/90" {...props} />
          ),
          td: ({ node, ...props }) => (
            <td className="px-4 py-3 border-b border-white/5 last:border-0" {...props} />
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
