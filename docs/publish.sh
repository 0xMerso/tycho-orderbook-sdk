mdbook build docs -d build
# mkdir -p ../tycho-orderbook-docs/docs
cd ../tycho-orderbook-docs
find . -mindepth 1 ! -name '.git' ! -path './.git/*' -exec rm -rf {} +
cd ../tycho-orderbook
cp -r docs/build/* ../tycho-orderbook-docs/
cd ../tycho-orderbook-docs
git add .
git commit -m "Update"
git push
