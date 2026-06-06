#include <locale.h>
void foo(struct lconv* bar)
{
	char *qux = &bar->int_n_sign_posn;
	(void) qux;
}
int main(void) { return 0; }
