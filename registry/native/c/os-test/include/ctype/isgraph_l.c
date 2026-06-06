#include <ctype.h>
#ifdef isgraph_l
#undef isgraph_l
#endif
int (*foo)(int, locale_t) = isgraph_l;
int main(void) { return 0; }
