#include <wchar.h>
#ifdef wcstof
#undef wcstof
#endif
float (*foo)(const wchar_t *restrict, wchar_t **restrict) = wcstof;
int main(void) { return 0; }
