import { BookmarkArtifact } from './types';

export const initialGraphData: any = {
  nodes: [
    { 
      id: 'org_nexus', label: 'Nexus Corp', type: 'organization', description: 'Central Headquarters', 
      date: '2024-01-01', milestone: 'Foundation',
      reason: 'Established to consolidate AI research efforts.',
      relatedFiles: ['/docs/charter.md', '/docs/vision.pdf'],
      decisions: ['Opted for a distributed architecture', 'Selected Rust as core systems language'],
      commits: ['ca82a6d - Initial repo setup', '1b9d4f2 - Add organization charter'],
      iterations: ['Search: "Optimal corporate structure for AI lab"']
    },
    { 
      id: 'proj_nova', label: 'Project Nova', type: 'project', description: 'Next-gen AI deployment', parentId: 'org_nexus', 
      date: '2025-05-15', milestone: 'Alpha Release',
      reason: 'Need for scaling context windows efficiently in production.',
      relatedFiles: ['/nova/architecture.excalidraw', '/nova/README.md'],
      decisions: ['Adopted Ring Attention mechanism', 'Shifted from GPU to TPU clusters for cost'],
      commits: ['4f81c9a - Add Ring Attention prototype', '901bca1 - Benchmark TPU inference times'],
      iterations: ['Iteration 1: Naive attention', 'Iteration 2: FlashAttention', 'Iteration 3: Ring Attention']
    },
    { id: 'sub_nlp', label: 'NLP Engine', type: 'subproject', description: 'Deep learning core', parentId: 'proj_nova', date: '2025-08-20', milestone: 'Beta Release' },
    { id: 'sub_ui', label: 'UI Sandbox', type: 'subproject', description: 'Frontend experimentation', parentId: 'proj_nova', date: '2025-09-01', milestone: 'Beta Release' },
    { id: 'sess_sprint14', label: 'Sprint 14', type: 'session', description: 'Evaluation of Q2 metrics', parentId: 'sub_nlp', date: '2026-04-10', milestone: 'RC1' },
    { id: 'proj_aegis', label: 'Project Aegis', type: 'project', description: 'Security protocol overhaul', parentId: 'org_nexus', date: '2026-01-10', milestone: 'Alpha Release' },
    { id: 'sub_sec', label: 'Zero Trust Network', type: 'subproject', description: 'Identity verification', parentId: 'proj_aegis', date: '2026-03-05', milestone: 'RC1' },
    { id: 'org_acq', label: 'Acquired Tech Inc', type: 'organization', description: 'Recent tech acquisition', parentId: 'org_nexus', date: '2026-05-20', milestone: 'Post-Launch' },
  ],
  links: [
    { source: 'org_nexus', target: 'proj_nova', relation: 'owns' },
    { source: 'org_nexus', target: 'proj_aegis', relation: 'owns' },
    { source: 'proj_nova', target: 'sub_nlp', relation: 'contains' },
    { source: 'proj_nova', target: 'sub_ui', relation: 'contains' },
    { source: 'sub_nlp', target: 'sess_sprint14', relation: 'discussed_in' },
    { source: 'proj_aegis', target: 'sub_sec', relation: 'contains' },
    { source: 'org_nexus', target: 'org_acq', relation: 'acquired' },
    { source: 'org_acq', target: 'proj_aegis', relation: 'collaborates' },
  ]
};

export const initialBookmarks: BookmarkArtifact[] = [
  { id: 'b1', type: 'Table', title: 'User Retention Metrics', date: '2026-06-07', category: 'Analytics', description: '', url: '', addedAt: '' },
  { id: 'b2', type: 'Graph', title: 'CPU Workload Array', date: '2026-06-06', category: 'System', description: '', url: '', addedAt: '' },
  { id: 'b3', type: 'Code Snippet', title: 'RAG Pipeline Script', date: '2026-06-05', category: 'Development', description: '', url: '', addedAt: '' },
  { id: 'b4', type: 'Data Card', title: 'System Diagnostics', date: '2026-06-04', category: 'System', description: '', url: '', addedAt: '' },
];
