test expect_positive() {
  let val = 5
  expect True = val > 0
  True
}

test expect_false() {
  let val = 5
  expect False = val < 0
  True
}
