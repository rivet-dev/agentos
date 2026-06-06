#include <termios.h>
void foo(struct termios* bar)
{
	cc_t *qux = bar->c_cc;
	(void) qux;
}
int main(void) { return 0; }
