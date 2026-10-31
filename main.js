// Grid background that slowly drifts on its own
(() => {
  const grid = document.getElementById('gridBg');
  if (!grid) return;

  const gridSize = 40;
  let currentX = 0;
  let currentY = 0;

  // Random direction chosen on page load (resets only on reload)
  let driftX = (Math.random() - 0.5) * 0.6;
  let driftY = (Math.random() - 0.5) * 0.6;

  function animate() {
    // Apply drift
    currentX += driftX;
    currentY += driftY;

    // Keep within bounds and bounce
    const maxOffset = gridSize * 3;
    if (Math.abs(currentX) > maxOffset) {
      driftX = -driftX;
      currentX = Math.sign(currentX) * maxOffset;
    }
    if (Math.abs(currentY) > maxOffset) {
      driftY = -driftY;
      currentY = Math.sign(currentY) * maxOffset;
    }

    grid.style.backgroundPosition = `${currentX}px ${currentY}px`;
    requestAnimationFrame(animate);
  }
  animate();
})();

// Tab functionality
document.querySelectorAll('.tab').forEach(tab => {
  tab.addEventListener('click', () => {
    const targetTab = tab.dataset.tab;
    
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    
    tab.classList.add('active');
    document.getElementById(targetTab).classList.add('active');
  });
});