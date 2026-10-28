// Child collectors must stop if the desktop/API process exits during a task.
const parent = Number(process.env.TAOSHU_PARENT_PID);
if (Number.isInteger(parent) && parent > 0) {
  setInterval(() => {
    try { process.kill(parent, 0); }
    catch (error) { if (error.code === 'ESRCH') process.exit(75); }
  }, 500).unref();
}
