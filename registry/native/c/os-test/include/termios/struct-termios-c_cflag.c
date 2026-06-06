#include <termios.h>
void foo(struct termios* bar)
{
	tcflag_t *qux = &bar->c_cflag;
	(void) qux;
}
int main(void) { return 0; }
