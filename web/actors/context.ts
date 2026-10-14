import { createContext, useContext } from "react";
import type { ActorsModel } from "./model.ts";

// The model of the snapshot and clock the page draws; one per render of ActorsPage, so
// every part of the page reads the same instant.
export const ActorsModelContext = createContext<ActorsModel | null>(null);

export function useActorsModel() {
  const model = useContext(ActorsModelContext);
  if (!model) throw new Error("The Actors page provides its model");
  return model;
}
