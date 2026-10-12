import { Router } from "express";
import { priceOrder } from "../services/pricing";
import { reserve } from "../services/inventory";

export const ordersRouter = Router();

ordersRouter.post("/", (req, res) => {
  const { items } = req.body;
  if (!Array.isArray(items) || items.length === 0) {
    return res.status(400).json({ error: "items requeridos" });
  }
  reserve(items);
  res.status(201).json({ total: priceOrder(items) });
});
