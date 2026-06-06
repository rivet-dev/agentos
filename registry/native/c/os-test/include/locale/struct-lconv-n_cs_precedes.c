#include <locale.h>
void foo(struct lconv* bar)
{
	char *qux = &bar->n_cs_precedes;
	(void) qux;
}
int main(void) { return 0; }
