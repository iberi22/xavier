import { openuiLibrary } from "@openuidev/react-ui";
import { DecisionCard } from "./components/DecisionCard";
import { ProjectCard } from "./components/ProjectCard";
import { QuestionCard } from "./components/QuestionCard";

export const combinedLibrary = {
  ...openuiLibrary,
  QuestionCard,
  DecisionCard,
  ProjectCard,
};
