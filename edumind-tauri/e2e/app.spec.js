import { expect, test } from "@playwright/test";

test("renders the shell, routes modules, switches theme, and exposes planner content", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Continue offline" }).click();
  await expect(page.getByText("One calm place for your study system.")).toBeVisible();
  await page.getByRole("button", { name: "Planner", exact: true }).click();
  await expect(page.getByText("Turn classes and focus blocks into the weekly routine every module shares.")).toBeVisible();
  await page.getByLabel("Block").fill("Calculus");
  await page.getByRole("button", { name: "Add block" }).click();
  await expect(page.getByRole("button", { name: "Edit Calculus" })).toBeVisible();
  await page.getByRole("button", { name: "Edit Calculus" }).click();
  await page.getByLabel("Block").fill("Calculus tutorial");
  await page.getByRole("button", { name: "Update block" }).click();
  await expect(page.getByRole("button", { name: "Edit Calculus tutorial" })).toBeVisible();
  await page.getByLabel("Timetable rows").fill("Wed 14:00-15:30 Physics lab");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Import timetable blocks" }).click();
  await expect(page.getByRole("button", { name: "Edit Physics lab" })).toBeVisible();
  await page.getByRole("button", { name: /Switch to dark theme/i }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: /Switch to light theme/i }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Learning Map" }).click();
  await expect(page.getByTestId("graph3d-canvas")).toBeVisible();
});
