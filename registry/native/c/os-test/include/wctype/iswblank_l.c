#include <wctype.h>
#ifdef iswblank_l
#undef iswblank_l
#endif
int (*foo)(wint_t, locale_t) = iswblank_l;
int main(void) { return 0; }
