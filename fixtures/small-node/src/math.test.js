const { add } = require('./math');

describe('math', () => {
  it('adds', () => {
    expect(add(1, 2)).toBe(3);
  });
});
