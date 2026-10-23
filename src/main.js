import { createApp } from 'vue';
import App from './App.vue';
import './styles.css';
import './ui.css';
import UiButton from './components/ui/Button.vue';
import SearchSelect from './components/ui/SearchSelect.vue';

createApp(App)
  .component('UiButton', UiButton)
  .component('SearchSelect', SearchSelect)
  .mount('#app');
