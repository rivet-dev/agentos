#include <wctype.h>
#ifdef iswgraph_l
#undef iswgraph_l
#endif
int (*foo)(wint_t, locale_t) = iswgraph_l;
int main(void) { return 0; }
