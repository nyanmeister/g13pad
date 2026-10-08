// The LCD page the Source engine renders for g15.so (g13pad's health feeder).
// Installed by `g13map health source DIR` as <mod>/custom/g13pad/resource/g15.res.
// The text items are the feed: "G13 wait" without a player, else the health line.
// m_lifeState prints as nothing while alive and as raw bytes otherwise.
"Logitech G-15 Keyboard Layout"
{
	"game"		"g13pad health meter"
	"chatlines"	"1"
	"page"
	{
		"titlepage"	"1"
		"static_text" { "size" "medium" "align" "left" "x" "0" "y" "0" "w" "160" "text" "G13 wait" }
	}
	"page"
	{
		"requiresplayer"	"1"
		"static_text" { "size" "medium" "align" "left" "x" "0" "y" "0" "w" "160" "text" "G13 %(localplayer)m_iHealth% 100 %(localplayer)m_lifeState%" }
	}
}
