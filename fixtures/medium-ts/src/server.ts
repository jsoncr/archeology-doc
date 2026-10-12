import express from "express";
import { ordersRouter } from "./routes/orders";
import { usersRouter } from "./routes/users";

const app = express();
app.use(express.json());
app.use("/orders", ordersRouter);
app.use("/users", usersRouter);

app.listen(Number(process.env.PORT ?? 3000));
