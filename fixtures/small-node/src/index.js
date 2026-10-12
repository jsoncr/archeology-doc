// Punto de entrada del fixture pequeño.
const express = require('express');
const { add } = require('./math');

const app = express();
app.get('/add', (req, res) => {
  const a = Number(req.query.a || 0);
  const b = Number(req.query.b || 0);
  res.json({ result: add(a, b) });
});

// TODO: servir estáticos
if (require.main === module) {
  app.listen(3000);
}
module.exports = app;
