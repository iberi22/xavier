import { BrowserRouter, Route, Routes } from "react-router-dom";
import { AgentManager } from "./components/AgentManager";
import { Dashboard } from "./components/Dashboard";
import { Layout } from "./components/Layout";
import { MemoryBrowser } from "./components/MemoryBrowser";
import { Settings } from "./components/Settings";

export default function App() {
  return (
    <BrowserRouter>
      <Layout>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/memory" element={<MemoryBrowser />} />
          <Route path="/agents" element={<AgentManager />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </Layout>
    </BrowserRouter>
  );
}
