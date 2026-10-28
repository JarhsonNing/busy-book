import { createApp } from "vue";
import App from "./App.vue";
import { modalFocus } from "./modal-focus";
import "./styles.css";
import "./ui.css";
import "./redesign.css";
import UiButton from "./components/ui/Button.vue";
import SearchSelect from "./components/ui/SearchSelect.vue";

createApp(App)
  .directive("modal-focus", modalFocus)
  .component("UiButton", UiButton)
  .component("SearchSelect", SearchSelect)
  .mount("#app");
