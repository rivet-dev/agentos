#include <termios.h>
void foo(struct winsize* bar)
{
	unsigned short *qux = &bar->ws_col;
	(void) qux;
}
int main(void) { return 0; }
