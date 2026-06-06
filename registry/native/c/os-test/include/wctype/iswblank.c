#include <wctype.h>
#ifdef iswblank
#undef iswblank
#endif
int (*foo)(wint_t) = iswblank;
int main(void) { return 0; }
