#include <locale.h>
void foo(struct lconv* bar)
{
	char *qux = &bar->int_n_sep_by_space;
	(void) qux;
}
int main(void) { return 0; }
