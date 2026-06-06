#include <wctype.h>
#ifdef iswprint
#undef iswprint
#endif
int (*foo)(wint_t) = iswprint;
int main(void) { return 0; }
