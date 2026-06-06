#include <locale.h>
void foo(struct lconv* bar)
{
	char *qux = &bar->n_sign_posn;
	(void) qux;
}
int main(void) { return 0; }
