import { Router } from "express";
import { db } from "../db/client";

export const usersRouter = Router();

usersRouter.get("/:id", async (req, res) => {
  const user = await db.findUser(req.params.id);
  // TODO: no se valida que el usuario solicitante tenga permisos
  res.json(user);
});
