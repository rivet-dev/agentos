#include <locale.h>
void foo(struct lconv* bar)
{
	char **qux = &bar->int_curr_symbol;
	(void) qux;
}
int main(void) { return 0; }
