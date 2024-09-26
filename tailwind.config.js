module.exports = {
  content: [
    './src/templates/**/*.{html,js}',  // Include all HTML/JS files in your templates
    './static/**/*.{js,css}',  // Include all CSS/JS files in your static folder
    './node_modules/flowbite/**/*.js'  // Flowbite plugin files
  ],
  plugins: [
    require('flowbite/plugin')  // Include the Flowbite plugin
  ]
}
