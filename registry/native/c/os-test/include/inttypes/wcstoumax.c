#include <inttypes.h>
#ifdef wcstoumax
#undef wcstoumax
#endif
uintmax_t (*foo)(const wchar_t *restrict, wchar_t **restrict, int) = wcstoumax;
int main(void) { return 0; }
