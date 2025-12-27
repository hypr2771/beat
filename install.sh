cargo build --release
sudo mv beat.service.example /etc/systemd/system/beat.service
sudo systemctl daemon-reload
sudo systemctl enable beat
sudo systemctl restart beat

mkdir -p yt-dlp
cd yt-dlp
curl https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -O
echo "PATH=$PATH:$(PWD)/yt-dlp" >> ~/.zshrc

source ~/.zshrc
yt-dlp -U
