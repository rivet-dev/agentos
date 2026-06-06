#include <wctype.h>
#ifdef iswalpha_l
#undef iswalpha_l
#endif
int (*foo)(wint_t, locale_t) = iswalpha_l;
int main(void) { return 0; }
