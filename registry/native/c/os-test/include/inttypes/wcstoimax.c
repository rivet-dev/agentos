#include <inttypes.h>
#ifdef wcstoimax
#undef wcstoimax
#endif
intmax_t (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstoimax;
int main(void) { return 0; }
