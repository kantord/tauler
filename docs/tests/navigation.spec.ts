import { test, expect } from '@playwright/test'

test('home → docs via the DOCS link', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(String(err)))

  await page.goto('/')
  await page.getByRole('link', { name: 'DOCS', exact: true }).click()

  await expect(page).toHaveURL(/\/docs\/$/)
  await expect(page.locator('h1')).toContainText('tauler')
  expect(errors).toEqual([])
})

test('browser back from docs returns to a working home page', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('link', { name: 'DOCS', exact: true }).click()
  await expect(page).toHaveURL(/\/docs\/$/)

  await page.goBack()
  await expect(page).toHaveURL(/\/$/)
  await expect(
    page.getByRole('link', { name: 'DOCS', exact: true }),
  ).toBeVisible()
})
