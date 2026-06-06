#include <ctype.h>
#ifdef isdigit_l
#undef isdigit_l
#endif
int (*foo)(int, locale_t) = isdigit_l;
int main(void) { return 0; }
