#include <wchar.h>
#ifdef ungetwc
#undef ungetwc
#endif
wint_t (*foo)(wint_t, FILE *) = ungetwc;
int main(void) { return 0; }
