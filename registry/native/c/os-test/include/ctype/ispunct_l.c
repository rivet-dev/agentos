#include <ctype.h>
#ifdef ispunct_l
#undef ispunct_l
#endif
int (*foo)(int, locale_t) = ispunct_l;
int main(void) { return 0; }
