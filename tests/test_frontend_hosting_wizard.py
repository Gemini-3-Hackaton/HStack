import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class FrontendHostingWizardTests(unittest.TestCase):
    def test_index_defines_hosting_wizard_and_settings_modal(self):
        html = (ROOT / "static" / "index.html").read_text()

        self.assertIn('id="hosting-wizard"', html)
        self.assertIn('id="settings-modal"', html)
        self.assertIn('id="settings-button"', html)
        self.assertIn('data-hosting-option="managed"', html)
        self.assertIn('data-hosting-option="self-hosted"', html)

    def test_app_persists_hosting_option_and_opens_settings(self):
        app_js = (ROOT / "static" / "app.js").read_text()

        self.assertIn("hstack_hosting_option", app_js)
        self.assertIn("persistHostingOption", app_js)
        self.assertIn("settingsButton.addEventListener('click'", app_js)
        self.assertIn("settingsSave.addEventListener('click'", app_js)


if __name__ == "__main__":
    unittest.main()
