#include <wctype.h>
#ifdef iswcntrl
#undef iswcntrl
#endif
int (*foo)(wint_t) = iswcntrl;
int main(void) { return 0; }
