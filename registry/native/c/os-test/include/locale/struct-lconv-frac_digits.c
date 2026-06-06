#include <locale.h>
void foo(struct lconv* bar)
{
	char *qux = &bar->frac_digits;
	(void) qux;
}
int main(void) { return 0; }
